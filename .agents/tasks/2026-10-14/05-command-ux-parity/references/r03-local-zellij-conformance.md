# Local zellij version conformance

2026-10-15. Read-only product research and task-owned executable/process probes
authorized after R02b; no production zellij owner or GUI integration implemented.

## Pin and provenance

Official release: <https://github.com/zellij-org/zellij/releases/tag/v0.43.0>.
The independently downloaded `zellij-no-web-x86_64-unknown-linux-musl.tar.gz`
has SHA-256
`a677d0a318ff01e0ee4a1c34315f1cb5731075ea3b40b587cb18c571723b2794`,
matching GitHub's release asset digest. Extracted executable SHA-256
`dec1863d0141a3971d52cfcd6d7cfab8ab7e94aed74773c09b8339c6660be446`
matches the release's published `zellij-no-web-x86_64-unknown-linux-musl.sha256sum`.
Its actual version output is `zellij 0.43.0`.

The exact-tag upstream [LICENSE.md](https://github.com/zellij-org/zellij/blob/v0.43.0/LICENSE.md)
is MIT, copyright 2020 Zellij contributors. Binary, license, release metadata,
published checksum, actual probe scripts/output and limited exact-tag upstream source
inspection are retained only under `/tmp/zmux-zj-0430-v7bhuqvn`.
Annotated tag object `2f825e87971fe44bd8e86a19e94cacea3d39ac52` resolves to release
commit `23bb542548d517f7f20a7fdd14a943f1a610df37`. The downloaded source archive
SHA-256 is `772826557e6d77feec261cf9ad64203ca8280be55a4ba3d13225c3075eafe98e`.
No upstream implementation, assets, plugins or binaries were added to this repository.
No global installation occurred. HOME and user configuration/session directories were
unchanged. The fixture explicitly redirected socket, config, cache, data, runtime
and temporary paths to private task-owned directories.

## Actual Linux observations

- `--config PATH --layout PATH attach --create-background NAME options
  --session-serialization false --on-force-close detach` starts a detached owner.
  A minimal single-pane KDL layout executes an explicit `/bin/sh SCRIPT` startup.
  An inline KDL trial initially failed parsing; the successful fixture uses explicit
  newline-delimited nodes. Layout values must be encoded by a real KDL writer or
  deliberately restricted/escaped format, not shell concatenation.
- `ZELLIJ_SOCKET_DIR` is a base directory: this version adds `/0.43.0/NAME`.
  A full `zmux-<UUID>` name fits within this short fixture's Unix socket limit.
  Both socket directories are mode 0700. This Linux binary creates the socket with
  mode 01700; private group/other access checks are appropriate, rather than copying
  tmux's exact mode-0600 socket requirement. macOS mode/path behavior remains untested.
- Two sequential actual PTY clients attached using explicit `attach NAME options
  --on-force-close detach --session-serialization false`. Each sent a shell command
  through its PTY; independent atomic output files contained the original shell PID.
  Shell PID **2874462**, Linux start ticks **1208620**, survived client PIDs **2874490**
  and **2874601** exiting after SIGTERM. Startup count remained exactly `x`.
  `/proc` records captured each real zellij client after exec and the server's exact
  exclusively owned socket argument. `pty-results.json` and raw terminal logs retain
  the observations. This is portable protocol evidence, not native AppKit acceptance.
- The fixture config intentionally sets `on_force_close "quit"`. A separate fresh
  unique-PID negative control omitted the client override: SIGTERM ended its shell.
  With the override, both positive clients exited cleanly and left their owner live.
  Thus the client override is necessary even when the owner was created detached.
- Exact `delete-session --force NAME` removed the owned server and shell. Final
  listing exited 1 with `No active zellij sessions found.` Cleanup affected only
  full-UUID names created by these probes. All task-owned zellij processes ended.

## Exact listing and client evidence

`list-sessions --short --no-formatting` includes exited names without a state field.
Full `--no-formatting` output explicitly reports `(EXITED - attach to resurrect)`.
Even with serialization disabled, session metadata directories can cause exited
records to appear. Existence in short output is therefore insufficient proof that
an attachment is live. The future owner should produce its own typed listing from
the reserved registry, exact private socket and server identity. Upstream's human
listing can remain diagnostic output. In pinned `zellij-utils/src/sessions.rs`,
`get_sessions` probes each socket with `ConnStatus`, accepts only an initial
`Connected` message, and can remove refused stale sockets; it is neither a structured
PID query nor a filesystem-read-only API.

`action list-clients` produced no stdout in successful observations and once failed
with `There is no active session!` while a later exact close still saw a live owner.
It is unsuitable as a client-PID confirmation API. Pinned
`SessionLayoutMetadata::list_clients_metadata` formats client ID, pane ID and running
command, rather than OS process identity; `ClientAttributes` carries size and style.
The future runtime must avoid claiming tmux's protocol PID/server-incarnation guarantees.

The later Linux acceptance instead combined independent kernel evidence:

- The owned shell's parent PID/start ticks and exact `zellij --server SOCKET`
  executable arguments identify its server. Connecting directly to the exclusively
  owned socket and reading `SO_PEERCRED` confirms that server PID and UID.
- A real PTY client's executable/PID/start ticks are recorded after exec and terminal
  output. `ss -xnpH` supplies both ends of its actual Unix connection. Matching the
  reversed client/server socket inode pair proves the client connects to that exact
  socket and server process. Internal socketpairs are explicitly excluded.
- A new serialized-state server has a different PID/start identity, so the same
  session name or a live child process alone cannot establish original-owner reuse.

`conformance2-results.json` records a complete successful matrix and kernel peer
links. These are research proofs; the product does not depend on `ss`. A production
acknowledgement implementation must preserve this exact-peer/identity invariant or
justify an equivalent fence within an exclusive per-record namespace. macOS needs
its own process/peer inspection and acceptance; Linux `/proc` evidence is not that gate.

## Serialization and resurrection controls resolved

Initial trials did not create serialized layouts. Pinned source explains two traps:
`PtyInstruction::LogLayoutToHd` skips layouts when `SessionLayoutMetadata::is_dirty`
finds only a default shell/base layout, and attach boolean flags are processed in
both `Setup::from_cli_args` and `commands::start_client`. The latter uses XOR in
`Options::merge_from_cli`. Reasoning from that XOR helper alone gives the wrong
effective configuration: setup has already merged the attach options first.

The final independent positive control uses config `session_serialization true`,
omits the serialization CLI flag, and keeps a nondefault `/bin/sleep 90` command
running. A real `session-layout.kdl` then appears. A matching dirty-command session
with the same config true and explicit `--session-serialization false` produces no
layout across three one-second serialization intervals. Config false plus CLI false
also produces none. Thus the explicit false path is established for this pinned
version; no version-wide claim is inferred from an option name.

After killing the independently serialized owner, full listing reports EXITED and
its exact socket disappears. A temporary read-only candidate launch guard refuses
the actual cache with reason `serialized_state`; its refusal leaves startup count
one and creates no server. Bypassing that guard with ordinary `attach NAME`, **without
`--create` or `--force-run-commands`**, actually resurrects a new server. Both direct
socket peer credentials and the PTY client's kernel peer link identify that new
server. `resurrection-no-force-results.json` records this separate control. Disabling
serialization on an attaching client therefore does not prevent revival of an
already existing cached layout. Attaching exited records must fail before spawn.

## Exact-name fallback guard

Pinned `attach_with_session_name` accepts a unique prefix. The real adversarial
fixture creates only `FULL_UUID_NAME-x`, then invokes `attach FULL_UUID_NAME`.
The child remains a real zellij client, but its reversed socket inode pair points
to the `-x` server. This independently proves that a successful child process and
explicit full-UUID argv are insufficient by themselves.

The candidate guard refuses `missing_exact_socket` before launching against that
fixture and independently verifies the shadow server remains untouched. The same
guard admits a live exact socket only when its UID/private permissions, exclusive
namespace, peer PID and process start identity match the reserved owner and no
serialized layout exists. This is a proposed policy exercised by a temporary
research script, not a delivered product API. A missing session with no cache and
no prefix shadow exits with `No session with the name ... found!` and creates no server.

## Recommended next scope

The Linux evidence is now sufficient to draft a headless owner design. Use a distinct
identity type and reserve the logical UUID before creation; retain uncertain start
outcomes without replaying commands. Give every record an exclusive short socket
namespace and isolated cache so prefix fallback and resurrection state cannot cross
records. Keep full UUID names, reject any unexpected namespace/cache state, and
validate actual server process identity. A short runtime directory must be designed
separately from long user configuration paths because zellij appends its version
directory and session name. Never inspect/delete unrelated user cache entries.

The design must specify pre/post-spawn exact-peer confirmation, replacement races,
headless supervision/exec semantics, cancellation/cleanup and macOS peer identity
before claiming a confirmed client. Do not reuse tmux's identity type or imply an
unsupported immutable server marker. After design approval, implement the private
headless owner with real owner-boundary tests, then typed GUI descriptor/receipt/
reopen integration and packaged macOS proof.
This single-client terminal architecture is closer to R02 than faithful `ssh-tmux`
control-mode mirroring, which needs a new remote stream backend and workspace/window/
pane topology mapping. An ordinary SSH command running `tmux attach` is not that
control-mode parity.

Follow-on [headless owner design](r03-local-zellij-owner-design.md) proposes a broker
with product-owned peer descriptors instead of cross-process socket inspection.
Its temporary opaque Linux relay proved real input/owner preservation and rejected
a wrong same-UID child. Public Darwin peer/process APIs and the native terminal
lifecycle remain explicit future conformance gates; no production owner is implemented.

## Isolated public-API transport fixture

The original standalone `tools/local-zellij-feasibility/run.py` now covers real
peer identity, two PTY attachments to the same shell, unrelated peer rejection,
bounded relay workers and confirmed daemon exit. Constructor failures clean up
PTY descriptors, unreaped children and broker workers; real missing-executable
and wrapper exec-failure controls verify this before fallback cleanup.

Independent parent source review and full Linux rerun passed at
`/tmp/zmux-zellij-parent-repaired-20261015/receipt.json` (SHA256
`04ae51743c503b1ae974110478de479e80600d7d1ee18363edb9bb4db9f36630`).
The author review run also passed, and removing constructor cleanup failed the
expected worker-lifetime assertion. The manual-only macOS public-peer workflow
remains unexecuted. This does not establish GUI attachment, production ownership,
job control, saturation behavior or Darwin support.

### First Darwin run and cleanup repair candidate

Run `38775157527` failed before SDK compilation: SIGKILL to the first completed
git command process group returned EPERM. The precise OS cause is not established.
The fixture now accepts this only on Darwin with the owned leader still unreaped
and WNOWAIT reporting exit, followed by a bounded public ps snapshot containing
that leader and exclusively zombie group members. Live, absent, malformed or
uninspectable groups still fail; other permission errors propagate. Final wait
reaps the child, and the source-command receipt retains the observed group.
Independent source review prompted explicit EPERM and unreaped-identity guards.
The complete Linux fixture still passes at
`/tmp/zmux-zellij-darwin-cleanup-reviewed-20261015/receipt.json`. Linux does not
execute this Darwin-only branch; hosted rerun is required.

Run `38775950132` at `df2e060` confirmed the initial EPERM group contained only
the unreaped leader in state `Z<`; SDK fixture compilation and public API loading
passed (macOS15.7.9, SDK15.5). It then failed Python peer executable identity.
The fixture now launches the current interpreter image obtained independently
from the public process API and compares that exact file; it does not trust a
peer-supplied path or skip image validation. This accommodates framework launcher
paths without weakening binary identity. Complete Linux rerun passes at
`/tmp/zmux-zellij-interpreter-parent-20261015/receipt.json`; Darwin remains pending.

### Directional relay regression

Run `38777058699` showed exec-stage111 with an empty PTY transcript, then a live
daemon after close-helper exit0. Relay receipts counted bytes read, not delivered,
and recorded ENOTCONN. A new real socket regression independently reproduced
queued upstream loss after reverse-side close: old relay delivered0/262144 bytes;
repaired relay delivers all262144 bytes with exact hash and zero upstream discard.
Socket shutdown errors are scoped to their direction; upstream backpressure and
a two-second reverse-close drain deadline remain. Distinct wrapper stage statuses
verify actual exec ENOENT without relying on transient PTY stderr.

Parent independent source review and complete pinned Linux run pass at
`/tmp/zmux-zellij-directional-parent-20261015/receipt.json`, with two real terminal
clients and original daemon exit confirmed. Author full run and old-relay negative
control also pass for their intended outcomes. Darwin cause/repair still requires
exact-source hosted execution.


### Darwin disconnected-peer limit and retained close design

Hosted run `38778452032` at `c33ec40` passed public credential/process checks,
exec-ENOENT status127 with an empty PTY diagnostic, the262144-byte directional
regression, and both actual terminal attachments with same-shell input, unrelated
peer rejection, TERM detachment and durable owner preservation. Cleanup still
failed: after the real `kill-session` helper exited, its second connection could no
longer provide a Darwin peer PID (`ENOTCONN`). Only its initial ConnStatus exchange
was authorized. The original daemon remained live; that run is not a cleanup pass.

An unreaped child reserves its PID but does not keep its disconnected Unix peer
queryable. Apple's public XNU `uipc_usrreq.c` requires a connected peer for
`LOCAL_PEERPID` and `LOCAL_PEERTOKEN`; this source observation explains the API
boundary but does not replace host execution. References:
<https://github.com/apple-oss-distributions/xnu/blob/main/bsd/kern/uipc_usrreq.c>,
<https://github.com/apple-oss-distributions/xnu/blob/main/bsd/sys/un.h>.

The fixture now closes through a third retained real PTY client. It waits for real
terminal readiness, verifies the pinned client executable, confirms input reached
the original shell PID and rechecks the original backend identity. Only then does
it send its explicit fixture-configured Ctrl+q Quit binding over the authenticated
live transport. The pinned Zellij implementation exits the server after its last
active client quits. All existing client TERM/durability cases remain. Physical
original-daemon exit, child reaping and joined workers remain required; no PID-only
kill or missing-credential fallback exists.

Short-lived `kill-session` through this Darwin broker is explicitly unsupported.
A future production administrative path needs a different supported lifetime or
owner transport; the retained Quit fixture cannot establish that missing path.


The final complete Linux run of the retained-close implementation passes at
`/tmp/zmux-zellij-retained-quit-final-20261015/receipt.json` (SHA256
`af95a15357d2e515cb68a7856fda7f956c4a1354277b57404c4b3743a7025a04`).
All seven harness input hashes match the source freeze. The third retained client
reported original-shell acknowledgement, readiness before Quit, exact client image,
zero upstream discard/pending bytes, exit0 and actual original-daemon exit. Both
existing TERM/durable-session cases and the directional/failed-exec checks passed;
scratch was removed after verified cleanup. The raw regression now retains its
connected child until the independent sink observes complete input, avoiding any
assumption about kernel buffer capacity keeping peer credentials available.

Restoring the pre-repair global relay-close behavior in a private source copy still
fails that raw regression with a child BrokenPipe,0/262144 sink bytes and no broker
I/O error. Receipt: `/tmp/zmux-zellij-retained-negative-20261015/receipt.json`.
Python syntax and scoped whitespace checks pass. These Linux results do not
establish Darwin close behavior; exact-source hosted execution remains required.

Parent independent full Linux rerun passes at
`/tmp/zmux-zellij-retained-parent-20261015/receipt.json`; independent source review
found no blocking lifetime/cleanup issue and verified all seven harness hashes.
Full `scripts/check` passed in a HEAD-plus-this-slice isolated snapshot with a
fresh Cargo target (`/tmp/zmux-zellij-retained-parent-check.log`). Retained-client
Darwin execution remains pending.


### Actual Darwin Quit and scoped terminal transport outcomes

Hosted `38779766351` at `58a882b` confirmed that the retained client reached the
original shell, sent Quit, exited0 and the original daemon identity actually ended.
The overall fixture still failed:59 opaque client bytes were queued when the backend
rejected a write during that termination. Their semantic contents are unknown; this
is not evidence of delivered cleanup messages. A subsequent real Linux run also
caught59 rejected bytes during explicit TERM attachment cancellation while the
original durable owner was preserved. Neither observation justifies ignoring active
stream input loss.

The broker now retains a typed rejection with errno, exact byte count/payload hash,
forwarded/discarded/pending counters and the terminal phase captured before the send.
Absent terminal proof, rejection still fails physical broker cleanup. Only an
explicitly requested owner Quit may classify post-request rejections after the
original full backend identity is gone and the owned child remains unreaped with
exit0. Explicit attachment cancellation has a distinct outcome requiring clean
unreaped child exit, the same original server and shell start identities, startup
count one and preservation checked again after joined transport cleanup. A phase
cannot be entered after prior failure, nor can later confirmation rewrite the phase
of an earlier rejection. Other errors remain fatal. Classified input is still
undeliverable and is never described as transmitted successfully.

The shared real-socket fixture now requests each terminal phase without its required
proof: live backend prevents owner-shutdown confirmation; missing clean child/shell
proof prevents attachment-cancel confirmation. Both retain unconfirmed loss and fail
broker cleanup before the expected negative result is recorded. The original
reverse-close262144-byte ordered delivery regression remains strict with zero
upstream discard. This changes cancellation/shutdown outcome semantics, not normal
active-stream acceptance or any full-parity gate.


Final complete Linux validation passes at
`/tmp/zmux-zellij-terminal-classification-freeze-20261015/receipt.json` (SHA256
`04b1c243d1c3db27f99128445184aa8890e20d886a112bd1d59b67a9a075b25d`).
All seven harness hashes match the freeze. This run actually exercised59 rejected
opaque bytes during successful original-owner Quit and retained their count/hash,
phase and `owner_exited_with_undeliverable_input` classification. The original
server exited, child0 stayed unreaped through confirmation, both TERM clients
preserved the original server/shell/startup and scratch was removed. The raw delivery
regression delivered262144 ordered bytes with zero upstream discard. Both new real
socket rejection cases remained fatal without their operation-specific proofs.

Private controls bypassed each confirmation proof independently. Both failed the
real socket check with `Live backend input loss was reclassified as completed
shutdown`, before starting any Zellij daemon. Receipts:
`/tmp/zmux-zellij-owner-proof-freeze-20261015/receipt.json` (SHA256
`b682ae674cba8d54595c123dbdf8689083a9a08a3ae824fad507726e1e3079d4`)
and `/tmp/zmux-zellij-cancel-proof-freeze-20261015/receipt.json` (SHA256
`a090059dd168df7448b7d16ab2695597eb43de971f71c97f0cbdf92b305cd35b`).
These controls changed only private copies. Python syntax/scoped whitespace checks
pass; exact-source Darwin validation of the classification remains required.


### Original foreground exit gate

Independent review found that earlier Quit receipts required the server and attached
client to end but did not inspect the original shell or its foreground wrapper before
scratch removal. The fixture now retains both full process identities at startup
and waits at most five seconds after authenticated Quit for both original identities
to disappear, before confirming terminal shutdown. The cleanup receipt separately
records server/shell/wrapper exit and scratch removal requires all three plus no
cleanup error. There is no adopted-PID signal or kill fallback. A still-queryable
original identity remains a failure; bounded public ps diagnostics distinguish a
visible zombie from a running process without silently declaring disappearance.
The earlier parent classification run is predecessor evidence only.

Final complete Linux validation passes at
`/tmp/zmux-zellij-foreground-exit-final-20261015/receipt.json` (SHA256
`88b69865e34ce410cd8f60e3f1e01d6a52e4c583166afe3840a8b49e1a752ea5`). All seven harness hashes match the
source freeze. The receipt separately confirms the original server, shell and wrapper
ended, the foreground wait completed before terminal confirmation, no cleanup errors
remained and scratch was removed. Both clean TERM-preservation cases, raw ordered
delivery, strict proof-negative cases and failed-exec guards passed. Python syntax
and scoped whitespace checks pass. Exact-source Darwin execution of this stronger
foreground-exit gate remains required; prior server-only receipts do not satisfy it.

Parent independent final real Linux run passes at
`/tmp/zmux-zellij-foreground-parent-20261015/receipt.json`: original server, shell
and wrapper exits confirmed, both cancellation-preservation cases pass, exact raw
transport and proof-denial cases pass, scratch removed. Independent reviewer
accepted the foreground-exit repair; no remaining blocker in this fixture scope.
Parent full repository checks also pass on the contemporary isolated T03 snapshot
(`/tmp/zmux-t03-parent-check.log`); this standalone Python fixture has no Rust/app
dependency changes. Exact-source macOS execution remains pending.


### Portable real write-rejection control

Hosted `38783041993` at `12588aa` stopped before starting Zellij: the negative raw
socket case timed out waiting for a backend write rejection after `shutdown(SHUT_RD)`.
Darwin accepted8192 bytes before backpressure and queued65536; the required rejection
was not observed during the five-second wait. A read-side shutdown is therefore not
a portable way to induce a write error. The classifier did not accept the missing
proof, and the earlier positive262144-byte delivery case passed.

The negative cases now keep the real client socket connected behind a bounded
private traffic gate. The fixture accepts the real backend connection, waits until
the actual broker has authenticated its kernel peer and full process identity, then
closes that accepted descriptor completely before releasing upstream traffic. The
backend's independently inspected process remains live. A real write to that closed
peer must produce a typed rejection; no transport error is injected or fabricated.
Both owner-shutdown and attachment-cancel cases still require the classifier to deny
completion and broker cleanup to reject unconfirmed input loss. The positive reverse
close case retains its strict ordered-delivery/no-upstream-discard requirement.
Classifier, terminal outcome proofs, process APIs and foreground-exit gates are
unchanged by this fixture repair.

Focused real socket cases pass at
`/tmp/zmux-zellij-authenticated-rejection-focused-20261015.json` (SHA256
`9b4cf4dfa45f2bf5932ddf8bbb7a3e213650751960e41abbb604c1c4c86744a5`). The complete pinned Linux fixture passes at
`/tmp/zmux-zellij-authenticated-rejection-20261015/receipt.json` (SHA256
`69dc3169847a87c7cce702f2c3c7a0f4fa917d45ac16c6083c0cde4ca3228be3`). All seven harness hashes match the source
freeze. Both negative cases record authentication before descriptor close and close
before traffic; proofs remain denied, original live backend preserved, errors/loss
retained and workers joined. The full run confirms original server/shell/wrapper exit
and scratch removal. Prior independent proof-bypass controls remain applicable because
the classifier was not changed. Exact-source Darwin execution remains required.


### Terminal exit drain and exact failure diagnostics

Hosted `38784160950` at `16cf61a` passed the corrected real raw rejection controls
and reached Zellij. Its first explicit TERM attachment failed the strict clean-child
check. Retained Quit then proved original server/shell/wrapper exit and the scoped
59-byte undeliverable-input outcome; overall run still failed. The prior receipt did
not retain the failed TERM child's actual waitid status or TERM-to-KILL escalation,
so its macOS root cause remains unconfirmed and no exit status was broadened.

A separate real PTY child independently reproduced an owner cleanup defect on
Linux: its TERM handler writes262144 fixed bytes before exit0, but the old owner
waited without reading the PTY, reached its three-second deadline and killed it
(exit-9, only19 readiness bytes retained). Draining PTY output while waiting at the
same TERM/KILL deadlines allows exit0 and keeps retained logs at65536 bytes. Receipts:
`/tmp/zmux-zellij-pty-drain-red-20261015.json` and
`/tmp/zmux-zellij-pty-drain-green-20261015.json`. This demonstrates the lifecycle bug;
it does not retroactively establish the unrecorded Darwin status.

The shared harness now contains that actual PTY-only regression, without any fake
Zellij server/protocol or extra production export. It drains during terminal exit
waits, preserves the strict child0 condition and records requested signals, actual
unreaped waitid PID/code/status, reaped outcome and byte counts. Every constructed
real client retains its bounded PTY log and broker lifecycle/counters even when
close fails before the ordinary success receipt. No sensitive environment is dumped.


The final complete Linux fixture passes at
`/tmp/zmux-zellij-pty-exit-drain-final-20261015/receipt.json` (SHA256
`5a873d717a1168764efe59aca84c31abf0579983f60267ee7ce515f50808bd4b`), with all seven harness hashes matching the
freeze. The real PTY regression exits0 after TERM only, drains258008 bytes while
retaining65536, and joins all workers. Both real Zellij TERM clients report actual
unreaped exit0 and reap0 without escalation; full original server/shell/wrapper exit
and scratch removal are confirmed. Restoring the old no-drain wait in a private
copy fails the new actual PTY regression before Zellij startup: TERM deadline,
SIGKILL escalation, waitid killed/status9 and reap-9 with19 bytes retained. Receipt:
`/tmp/zmux-zellij-no-exit-drain-20261015/receipt.json` (SHA256
`de25fc3b18d81c3a82da080f7b3bf75fa02e73d9bdc2dca24d588d2560e42459`). Python syntax/scoped whitespace checks pass.
The new diagnostics and drain require exact-source Darwin validation; no successful
exit status, preservation check or input-loss classification was loosened.


### Read-only exit drain

Parent source review found that the exit drain still called the ordinary terminal
query responder. Its blocking master write could stall if a terminating child emitted
query-like bytes after it stopped reading stdin. Exit waits and the post-Quit drain
now call the same bounded pump with responses disabled; ordinary readiness retains
query handling. The exit path performs no PTY writes, only a select with a30ms bound,
one bounded read and capped log retention. This removes the blocking-response path
rather than widening a timeout or accepting a different exit status.

The actual PTY-only regression disables canonical/echo handling, queues ASCII q input
with bounded nonblocking writes until EAGAIN, restores blocking master mode and sends
TERM. Its child defers input reads, emits query-like bytes in each output block, then
independently inspects queued stdin through a bounded500ms quiet observation. Since
seeded input contains no ESC, any ESC is a query response and exits3; only no response
allows exit0. This detects the unwanted response even if initial kernel backpressure
was transient. The receipt retains actual queued input, observed EAGAIN, read/retained
output counts, exact child outcome and close duration. The existing eight-second
watchdog also bounds a genuinely blocked responder. No test seam or invented Zellij
protocol is involved.

The focused actual PTY case passes at
`/tmp/zmux-zellij-exit-readonly-observer-focused-20261015.json`:11776 queued input
bytes, exit0 after TERM only,262162 output bytes drained,65536 retained and joined
workers. A private copy restoring query answers only during exit fails the same real
case with child exit3, independently detecting a reply:
`/tmp/zmux-zellij-exit-query-observer-control-20261015.json`. Initial controls relying
only on EAGAIN did not distinguish the old responder and are not accepted evidence.
A separate broad private control hit an unrelated preexisting credential-child send
race and is likewise excluded. Its observed failure is reported separately; no
credential fixture behavior was changed in that initial slice; the subsequently
authorized repair is recorded below.

Python syntax and scoped whitespace checks pass. The earlier full Linux run of the
initial read-only drain is predecessor evidence only; parent independent full Linux
and exact-source Darwin validation remain required for this final observer revision.


### Credential rejection before payload

Parent independent final Linux validation reproduced the earlier credential race:
the unrelated real child connected, the parent independently verified its peer PID,
UID and executable, rejected its different PID and closed before its send. The child
then exited on BrokenPipe even though the authorization boundary behaved correctly.
This is rejection-before-payload, not evidence that an owned peer may fail silently.

The child now reports a bounded typed send/receive outcome. Only the unrelated role
may report send EPIPE/ECONNRESET and it must still observe EOF; owned send errors and
all other send failures propagate. The parent validates that outcome only against its
own exact kernel-verified child PID/UID, expected independently owned PID and explicit
wrong-PID rejection record. An owned child still requires successful send and ACK.
Child exit0 or socket close alone is insufficient. Reports read at most4096 bytes and
contain no environment or secret state.

A private100ms delay before the unrelated send forces the real early-close ordering.
Pre-repair code fails with actual BrokenPipe and a correctly rejected parent record;
post-repair code passes with sendEPIPE/receiveEOF and the matching parent proof. A
separate private control keeps the actual reject/close but falsely marks its parent
record accepted; it fails `Closed peer lacks the matching independent authorization
rejection`. The ordinary focused current-source credential case also passes. No
production scheduling delay, injected socket error or authorization weakening was
added. Receipts and SHA256:

`/tmp/zmux-zellij-early-reject-red-20261015.json`
`3305d5919d0846a356c2f9e7e36904f33536dbe08fe53e4cf29c2fa55437511d`.

`/tmp/zmux-zellij-peer-outcomes-focused-20261015.json`
`92352f2d673b3107cd3e4f3c0ae2e1c6fc2a35621646a623965ad2b84bbc9a8c`.

`/tmp/zmux-zellij-early-reject-green-20261015.json`
`f9d23c5ca8f9aaf6d7a4543eb491a5a78170f3722dbb94b0efcad8a720d59420`.

`/tmp/zmux-zellij-early-reject-false-proof-20261015.json`
`e65113b369bd9a3ec70763922c5e0684aa00f93f2ca8601f64aeca924426bf1f`.

Python syntax and scoped whitespace checks pass. Final parent full Linux and
exact-source Darwin validation remain required for the combined frozen fixture.

Independent parent full Linux run passed the combined read-only exit drain and
credential-close repair: `/tmp/zmux-zellij-credential-close-parent-20261015/receipt.json`
(log `/tmp/zmux-zellij-credential-close-parent.log`). Source review verified the
unrelated child's close is accepted only alongside the parent's exact authenticated
rejection; owned peers still require ACK. Exact-source Darwin execution remains open.
