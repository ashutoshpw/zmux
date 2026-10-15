# F04 native default Session shutdown diagnostics

2026-10-15. Native engine opt-in work is paused; the default Session shutdown
gate must pass before native integration resumes. No engine default, production
cleanup behavior, assertion or deadline changes in this diagnostic slice.

Native runs `38815234251` and `38816029486` fail the same two actual Session
tests before app compilation: graceful output-flood cleanup returns `Timeout`,
and the isolated SIGCHLD-ignored cleanup-outcome regression has not finished at
its unchanged3s bound. Foundation `38815234305` independently fails the latter
on macOS; its graceful test and the Linux job pass. These are unresolved native
failures, not permission to suppress cleanup errors or increase deadlines.

The manual-only `Session shutdown diagnostics` workflow runs on macos-15 with
Rust1.95.0 and Python3.13. Its small driver retains exact source copies and hashes,
OS/toolchain identity, bounded complete logs, and all phase results. It runs a raw
owned-child probe followed by the actual default Session shutdown integration
tests both parallel and serialized, even if an earlier phase fails. No Ghostty
build or native app build is involved. Source hashes are rechecked after all
phases; retained source copies are checked against their initial hashes.

The raw probe observes ignored SIGCHLD across exec using actual finite children,
plus a controlling-PTY output flood that ignores TERM and is escalated to KILL
only while its physical child is unreaped. Both parent master descriptors close
before the final wait sample. Raw Python `os.waitid`, `getpgid` and `waitpid`
results/errors are recorded; missing Python waitid support is explicitly marked
unavailable. Python waitid is not claimed to validate Session's Rust rustix ABI.
The flood probe refuses to spawn unless its parent has default SIGCHLD disposition;
it does not reset signal policy or assume an auto-reaped PID remains reserved.
The ignored-SIGCHLD case never signals a numeric PID, because automatic reap may
already have released it. Observations stop signalling after reap or ECHILD.

The two unchanged failing Session assertions now include small public snapshots,
policy/budget context, worker completion, group lookup and nonreaping child
observations on failure. No viewport contents are dumped. The actual native
receipt must distinguish a fixture assumption from a production wait/cleanup
failure before any repair is chosen.

Local driver command: `python3 -B tools/session-shutdown-diagnostics/run.py
--output target/agent-validation/session-shutdown-diagnostics-linux-reviewed-20261015`.
Local portable execution does not establish Darwin semantics. Exact final
receipt, source freeze and read-only review are handed to the parent for
publication; the parent owns staging, commits and native dispatch.

## First exact Darwin diagnostic result

Run `38817229977` at `c9404cfb2d66d0a1d327d3f4e1edae061c495c2a`
completed with failed test phases on Darwin24.6 ARM. Artifact `12010813814`, ZIP
SHA256 `60c1f09836451ef5d9ceb9d2e8c235c7f0a4948656c1b8d53f7f777dfe747198`,
retains46 source inputs, complete raw logs and receipt. All46 retained copies
match both the receipt hashes and exact Git source. Local retention is under
`target/agent-validation/session-shutdown-diagnostics-darwin-38817229977/`.

The ignored-SIGCHLD raw child retains disposition1 after exec, but Darwin still
provides `waitid` exit status0 and actual `waitpid` reap of that exact child. Thus
SIG_IGN alone does not induce the required cleanup failure on this host. In both
parallel and serialized actual Session tests, the failing regression reports
`Exited`,7 bytes read, exit0 and no failure; its child is already reaped and its
worker correctly retains exited history. Waiting for worker completion without
closing is therefore the wrong fixture precondition on Darwin. The cleanup-error
assertions must remain: a replacement fixture needs an actual kernel ECHILD
failure before either shutdown policy can be assessed.

Both actual graceful output-flood cases pass in both focused test modes. The raw
PTY flood ignores TERM, sends KILL while unreaped, closes both masters, then
observes signaled9 and reaps its exact child. These passes do not resolve the
previous repeated full-job graceful Timeout; the enriched full native run is a
separate source of evidence. No production cleanup repair follows from this run.

The enriched full native run `38817223146` on the same exact commit also passes
the graceful deadline test and fails only the cleanup fixture, with the same
`Exited` history snapshot. The earlier intermittent graceful Timeout remains
unexplained; these passing observations are not a claimed fix.

## Kernel-fault fixture repair

The exact isolated test child now installs and verifies SIGCHLD `SA_NOCLDWAIT`
after exec. Its small `publish=false` support crate is a dev dependency only;
Session retains workspace `unsafe_code=forbid`. The helper denies unsafe elsewhere
and allows only its documented installation/check. It refuses installation
without the exact cleanup-test arguments and owned-child environment marker.
This controls the actual kernel fixture process; no production error injection
or Session behavior changed. Existing dependency pins remain unchanged.

The original3s completion bound and both policies' first/repeated cleanup-error
assertions remain. An added nonreaping ECHILD assertion proves the real kernel
fault, and successful captured child stderr is retained in `--show-output` logs.
The diagnostics and retained linkage source inventories recursively include the
new helper; the focused diagnostics now hash48 source inputs.

The exact local driver under
`target/agent-validation/session-shutdown-sigaction-linux-20261015/` passes raw
probe and all3 parallel plus3 serialized cases. Its log records actual ECHILD
under both policies. Scoped strict Clippy, formatting and whitespace checks pass.
All19 default Session behavioral tests pass. The same repaired fixture against
exact old `b831fac` production Session source fails on the original false Kill
shutdown success after actual kernel ECHILD. The negative control's source,
receipt and log remain under
`target/agent-validation/session-shutdown-sigaction-negative-control/`.
The native result below verifies actual Darwin ECHILD and preserved cleanup errors.

## Exact Darwin kernel-fault result

Run `38818223456` at `74a2287803e35397c5aed40f246985fd45874b01`
succeeds on Darwin24.6 ARM. Artifact `12012245753` is89481 ZIP bytes, SHA256
`1479d136a21e42b08d64a10509f6120ff012ffc8b6e4e4fd078e5c50521a97c7`;
the producer receipt SHA256 is
`229fac635ffd9e27982e6e76a4ad46a746681ff7e2482505a0681bfd92765468`.
All48 retained source copies match their receipt and exact Git source. The raw
probe, all3 parallel cases and all3 serialized cases execute successfully with
complete logs and no phase timeout. Local retention is under
`target/agent-validation/session-shutdown-diagnostics-darwin-38818223456/`.

Actual kernel ECHILD is recorded for Kill child12019 and Graceful child12022 in
the parallel run, and Kill child12027 and Graceful child12028 in the serialized
run. Each policy retains its original first and repeated shutdown cleanup-error
assertions. The kernel-fault fixture therefore now exercises its intended error
on Darwin as well as Linux. This does not claim a production repair for the
older intermittent graceful Timeout, nor native app/default engine acceptance.
Independent review verifies the live successful run, exact artifact/receipt and
all48 Git source hashes, both3-case modes and four actual ECHILD child identities:
`/tmp/zmux-session-shutdown-darwin-38818223456-independent-review.json`, SHA256
`41175235a3404a78d06b95a872cdf10886fcdc7b1e2a6397bf52442693ef5049`.
The fixture repair is accepted within that scope. Older graceful Timeout and
native app/default-engine gates remain separate.
