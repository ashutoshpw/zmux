# Foreground zellij owner and native acceptance

2026-10-15. The approved foreground owner is implemented and passes focused Linux
checks. Production Rust macOS acceptance remains pending. The passed public-API C
fixture is recorded in [version conformance](r03-local-zellij-conformance.md).

## Scope and public consumer

The actual CLI owns `local-zellij start NAME --headless [--cwd PATH] [--command TEXT]`,
`attach --headless --id UUID`, `list`, and `status --id UUID`. Start reserves a fresh
owner once and enters the same foreground attachment path used by later attach.
Unsupported detached, GUI, adoption and close/prune forms reject before owner setup.
Status reports `identity_present` or `uncertain`, never semantic attachment or
protocol liveness. No background service, arbitrary socket import or global PID signal.

Use the explicitly selected installed Zellij0.43.0 executable. Product execution
checks its canonical executable path and exact bounded version output. The separate
host gate downloads and verifies the pinned official no-web archive and binary;
product execution does not claim checksum verification of a user's installed binary.
No Zellij implementation, plugin, asset or binary is imported into product source.

## Reservation and actual workload bootstrap

A private mode0700 profile contains a mode0600 bounded atomic registry: at most64
records and128KiB, with a bounded OS lock. A fresh UUID reservation precedes one
creation attempt. Persisted states are `reserved`, `starting`, `bound`, `uncertain`.
Any uncertainty after possible execution retains the reservation and forbids replay.
No automatic retry, adoption, stale-record prune or owner termination is implemented.

Each record derives its short private runtime from the UUID under canonical `/tmp`
(`z<UUIDsimple>`), with isolated socket/config/cache directories. The relay uses that
same short base for its independent random namespace. Darwin resolves `/tmp` to
`/private/tmp`; the full version/session socket path is 97 bytes, below the existing
99-byte limit. The per-user system `TMPDIR` can exceed the limit and is not used
for these socket namespaces. The base must be a directory owned by root or the
current user; a group/other-writable base must have the sticky bit. Random roots
still use exclusive mode0700 creation and existing inode/UID binding. No global
environment variable is changed.
Zellij appends `s/0.43.0/zmux-<full UUID>`. Paths are bounded below the portable Unix
socket limit. Private type/mode/UID, root/socket device+inode, exact config, no prefix
shadow, no symlink/resurrection layout and bounded cache walk are revalidated.
Attach never creates missing runtime storage. Missing storage is uncertainty.
Private reads use O_NOFOLLOW|O_NONBLOCK before requiring a regular single-link private
file, so a replaced FIFO cannot block before type validation.

The transient bootstrap descriptor contains actual starter identity, current CLI
path, random64hex challenge, cwd/shell and bounded optional command. It is never
saved in durable registry metadata. The transient layout starts the actual production
`zmux __local-zellij-supervisor DESCRIPTOR`, not a fixture wrapper or shell ACK.
The layout includes only internal paths, explicitly restricted to UTF-8 without
control characters and escaped for quotes/backslashes. Startup text remains JSON.
The ordinary tab template contains only `pane`, without the supervisor/start command.

1. The supervisor connects to the private bootstrap listener, verifies the actual
   starter socket identity, sends its one-use challenge and waits.
2. The starter independently obtains the connected supervisor's kernel PID/UID and
   full identity. It requires the current CLI executable.
3. The starter connects to the new backend and independently reads its kernel peer
   identity. It requires the pinned Zellij path and kernel-observed supervisor PPID
   equal to that exact server PID, including a fresh full identity comparison.
4. The starting binding and namespace/socket inodes are persisted. Pending foreground
   cancellation is checked before sending `spawn_once`.
5. The supervisor spawns the requested workload once with inherited terminal IO.
   The starter independently reads its full identity and direct PPID/UID, persists
   `bound`, then sends `committed`. Startup JSON/layout/socket are removed.
6. The supervisor retains and finally reaps that physical workload. A bootstrap
   disconnect after spawn does not kill or replay it, even if commit became uncertain.

The registry stores name/UUID/state and full observed process identities, including
executable paths as identity evidence. It stores no startup shell text, environment,
credentials or arbitrary socket path. Future GUI identities must remain descriptors.
Fast-exiting or exec-changing workloads can produce truthful startup/attach uncertainty;
identity equality is not silently weakened to PID or same UID.

## Identity and physical child lifecycle

`crates/session-runtime/src/child.rs` owns shared process identities and fixed-child
pre-reap observation. Identity includes PID, UID, PPID, start sec/usec or Linux ticks,
boot UUID and executable path. It double-samples process fields around executable
lookup and rejects changed/malformed observations. Linux uses bounded public `/proc`.
Darwin uses reviewed safe wrappers over public proc/sysctl APIs. Connected peer
identity always comes from the actual descriptor, never an expected substituted PID.

`rustix` fixed P_PID waitid with EXITED|NOHANG|NOWAIT retains physical child ownership;
STOPPED/CONTINUED are requested for job control. ECHILD remains an error. Rustix does
not expose a returned si_pid accessor here, so no kernel-returned-PID receipt is
invented. The exact P_PID request and retained Child are the ownership binding.
Existing Session polling shares this observation. Final PTY/IO drop and reap happen
before entering history review, fixing the independently reproduced history zombie.

The foreground CLI owns the actual inherited terminal, saves termios/group, creates
one child process group and transfers the foreground TTY through safe APIs. It never
consumes terminal input. Signal-hook polling handles TERM/HUP/INT/WINCH/TSTP/CONT;
TSTP restores the caller terminal before stopping itself, then resumes/reassigns the
owned client. Startup cancellation is polled during version/launcher capture, before
workload authorization, and before attachment. Cancellation deadlines escalate only
the retained physical client group. No adopted server/workload is signaled.

## Opaque broker and truthful exit receipts

One worker owns each attachment's private Unix listener, at most4 simultaneous
connections and64KiB per direction per connection. It waits for the parent-published
exact child identity before admission. A wrong actual PID/UID is denied before
backend connect; every backend descriptor must match the reserved full server identity.
After validation the connected descriptor is retained, without retry/retargeting.
Nonblocking reads/writes preserve ordering and partial writes; full queues apply
backpressure. Half-close and reverse-close drain are bounded. Workers are joined
before the one final client reap, including early failure/drop paths. Cleanup checks
root, nested directory and broker socket inodes before deleting its own namespace.

Rejected upstream bytes retain count, SHA256, errno and the cancellation phase read
before the physical send attempt. Contents are opaque and never interpreted or logged.
Only explicitly requested cancellation, actual client exit0, joined workers, no pending
input and independently preserved original server/supervisor/workload can permit that
precise benign terminal outcome. No unknown bytes are claimed delivered. Natural
workload exit with rejected input remains an unknown failure even with client exit0.
The final transport receipt reports actual child exit, cancellation, preservation,
joined workers and accounting. It is not an early semantic `attached` receipt.

Pinned0.43.0 source shows list-clients metadata contains client/pane/program, not OS
PID or an owned-child acknowledgement. ConnStatus/Connected is server liveness, not
terminal attachment. The ordinary upstream human listing can also mutate stale
sockets and has asynchronous empty-output races. No helper listing advances product
state. Short-lived administrative helpers are unsupported on Darwin: LOCAL_PEERPID
can disappear after disconnect while the child remains unreaped. Never normalize
missing credentials, copy private kernel layouts or infer the expected peer PID.
The inspected pinned IPC byte path uses read/write and local dup, with no descriptor
passing or peer-PID-sensitive authorization. Revalidate on upgrades.

## Dependencies and provenance

Workspace `unsafe_code = "forbid"` remains intact. Reviewed exact dependencies:

- `rustix = 1.1.5`, process feature: safe Apple NOWAIT waitid, fixed P_PID.
- `darwin-libproc = 0.2.0`, MIT OR Apache-2.0: task_all_info rejects any count other
  than exact232-byte structure size; public pbsd exposes PID/UID/PPID/start sec/usec.
  Bounded pid_path preserves non-UTF8 PathBuf. The old experimental crate needs
  native SDK/runtime acceptance. Its memchr~2.3 conflicts with vte's^2.7.4. The
  approved minimal vendor patch changes only that manifest requirement to^2.3;
  source/FFI/exact-size checks remain unchanged. `vendor/darwin-libproc/UPSTREAM.md`
  and original licenses retain provenance. Sole upstream memchr use retains its API.
- `sysctl = 0.7.1`, MIT: only safe typed Ctl::new("kern.bootsessionuuid").value()
  String, validated as nonnil UUID. Never generic value_as<T>. The fixed Apple key
  is bounded; this does not approve arbitrary kernel-length allocation elsewhere.
- `signal-hook = 0.4.5`, MIT OR Apache-2.0, defaults disabled, iterator only: bounded
  signal slots/self-pipe, safe polling and unregister/drop, no extra signal thread.
- `sha2 = 0.10.9`, MIT OR Apache-2.0: streaming Digest for opaque loss, no asm feature.

Reviewed archives under `/tmp/zmux-r03-safe-api-20261015`:
darwin-libproc `cc629b7cf42586fee31dae31f9ab73fa5ff5f0170016aa61be5fcbc12a90c516`;
darwin-libproc-sys `ef0aa083b94c54aa4cfd9bbfd37856714c139d1dc511af80270558c7ba3b4816`;
sysctl `cca424247104946a59dacd27eaad296223b7feec3d168a6dd04585183091eb0b`;
signal-hook `b29ffa27d3d59a049bb328258de100f8012848d1b08d444e08bbc02ac468435e`;
sha2 `a7507d819769d01a365ab707794a4084392c824f54a7a6a7862f8c3d0892b283`.
License inventory now includes vendored path packages; notices were regenerated.

## Executed Linux checks and remaining gates

The real production CLI owner test uses the checksum-pinned executable and actual
portable-pty terminal. It proves kernel server→supervisor→workload lineage, workload
input, resize, startup count1, INT/TERM/HUP detach, TSTP/CONT restoration/resumed input,
status and multiple attachments to the same identities, and actual termios restoration.
It rejects changed config, resurrection layout, prefix shadow, replaced backend inode
and missing runtime at public attach; every denial preserves the original workload.
Queued TERM during a real registry Flock wait cannot authorize a workload.

Always-on CLI tests reject unsupported grammar, changed bootstrap starter and an
unauthorized spawn instruction on real sockets. Broker-owner real socket tests prove
wrong-child rejection before upstream connect, changed-backend rejection before input,
forced64KiB backpressure, ordered1MiB half-close drain, actual EPIPE rejection and
published SHA256 vector with pre-send cancellation phase. These use no Zellij protocol
mock and add no test-only production exports.

Shared child2 plus lifecycle14 tests passed independent review. The history zombie
regression has intended red/green logs `/tmp/zmux-r03-history-reap-{red,green}-20261015.log`.
A production supervisor FIFO regression likewise failed before O_NONBLOCK and passed
afterward: `/tmp/zmux-r03-fifo-{red,green}-20261015.log`. Independent public list
verification reproduced the FIFO hang and then refused it in0.023s after repair.

`.github/workflows/local-zellij-owner.yml` is a separate manually dispatched Linux/
macOS15 production gate. `run-owner.py` reuses the checksum-pinned download helper and
runs production shared-child, broker and actual CLI tests, rather than recreating an
owner in Python. Artifacts retain source SHA, host/toolchain, download hashes, pins,
notices and bounded test logs. Native production Rust compile/runtime acceptance is
pending; a cross-build or the earlier C fixture does not replace it.

The real SIGINT regression failed before adding its pre-send cancellation phase and
deadline, then passed through the common INT/TERM/HUP owner path:
`/tmp/zmux-r03-sigint-{red,green}-20261015.log`.

Still open: native production runtime gate; client-crash/deadline variants;
later ordinary-tab startup-once proof; broader resource/load and all platform coverage.
Host-test failures kill only their physical frontend; isolated fixture workloads are
finite with a20-second read timeout. The Cargo watchdog does not prove daemon cleanup,
and hosted runner cleanup is not a product close receipt. Generic close/prune requires
its own supported retained authenticated path and original server/workload termination
proof. GUI semantic attachment, migration/reopen and detached start are later slices.

Passed C fixture run38787914932 on ARM64 macOS15.7.9/SDK15.5 at
11258fead00c71dc24652280504b82def45307b9, receipt SHA256
3622aac43d58720ee17c3b629d5c13d6bcc4968816afcb02e35c1e3d1c294461,
proves its documented live-peer/TERM/retained-Quit fixture only. It does not establish
production foreground job control, safe Rust wrappers, registry policy or GUI parity.

First production host run38797306939 at adf51320a80e10db85bd8c9b2c01e1e42d268dbd
passed all24 focused Linux checks. ARM64 macOS15.7.9/SDK15.5 compiled the safe
identity/wait wrappers and passed live identity, then the existing stopped-child
regression exposed a nonterminal Stop result from exit-only observation. The shared
owner now filters Stop/Continue unless job-control observation was requested; no
consumer may treat suspension as exit or enter final wait on that observation.
Native/Foundation compilation also exposed an incorrect nix getpeereid module path;
the safe pinned API is nix::unistd::getpeereid with existing user+socket features.
Both repairs require fresh native execution; broker/CLI Darwin tests were not reached
in the failed run. Downloaded host receipts/logs are under
`/tmp/zmux-r03-hosted-38797306939`.

Repair validation also caught a real pre-exec workload admission race: a bound
record stored the inherited CLI executable for workload PID2416234, then the same
PID/start/PPID independently appeared as `/usr/bin/bash`. Startup now waits within
the existing deadline for the canonical selected shell executable, while requiring
unchanged PID/UID/PPID/start/boot and polling foreground cancellation. The actual CLI
owner test additionally requires that exact selected executable in its bound record.
Fast-exec commands and nonstandard executable shell wrappers that never expose that
selected image remain uncertain with no replay. Legitimate later user exec changes
also remain an attach/preservation semantics gap; no same-PID relaxation is implied.
The socket rejection fixture now explicitly shuts down its physical endpoint before
dropping its descriptor, so concurrent forks cannot keep a shared endpoint open and
invalidate the intended real EPIPE control.
Each actual attachment/input probe now writes a fresh nonce and requires exact
marker contents before signaling that client, preventing delayed input from a prior
attachment from falsely establishing readiness. Signal failures retain bounded final
PTY output for diagnosis; successful clean detach remains required.

Darwin runs38799167366/38799167465 and owner run38799176839 next exposed Unix
socket paths over SUN_LEN, first in the direct supervisor fixture and then the real
broker peer fixture. The production owner and relay now share checked canonical
`/tmp` selection. The direct auth fixture uses a short private root; the public
owner fixture injects a deliberately long private `TMPDIR` into its CLI subprocesses,
without changing the test process environment. Existing full foreground lifecycle
coverage fails before the repair with the production 99-byte rejection, and passes
afterward while retaining exact workload identity, detach and reattach assertions.
The existing real-kernel broker peer test likewise reproduced SUN_LEN under long
TMPDIR and passes after repair; the CLI auth/FIFO/grammar tests pass unchanged in
behavior. Queued cancellation still cannot authorize a workload under the long
subprocess TMPDIR. The pinned Linux Zellij0.43.0 binary checksum matches pins.json.

The isolated `f14ceee` baseline plus this five-file repair passed full `scripts/check`
(280 Rust tests, formatting, Clippy, inventory and foreign C ABI). Logs are
`/tmp/zmux-zellij-short-root-{red,green,owner-red,owner-green,cli,cancel,check}.log`.
Source review preserves exclusive mode0700 creation, shared-base sticky/owner checks,
namespace inode/UID fencing and existing 99-byte refusal. Fresh Darwin execution is
still required; this portable repair does not establish native reopen acceptance.
