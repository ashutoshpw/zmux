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
