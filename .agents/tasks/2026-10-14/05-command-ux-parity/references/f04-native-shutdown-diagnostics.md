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
