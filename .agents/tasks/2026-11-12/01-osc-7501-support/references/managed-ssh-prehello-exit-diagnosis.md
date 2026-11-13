# Managed SSH pre-Hello exit diagnosis

**Date:** 2026-11-13
**Source:** `2efb76d9bcbb9e0a00015602b8bcaf81c00920de`
**Hosted evidence:** The supplied universal development job log is
`/tmp/zmux-osc7501-final-2efb76d/universal-failed-job-131826686970-full.log`.
The same-source Foundation summary is
`/tmp/zmux-osc7501-final-2efb76d/foundation-summary.json`.

## Verdict

The logs prove that the managed lifecycle SSH child closed stdout before the
owner accepted `Hello`; they do not identify why. The owner redirects child
stderr to null and does not record the child exit status in its read loop. Its
only EOF diagnostic is the `hello`/`active` pair. `EPERM` is logged by process
group cleanup after the attempt returns and is not evidence that it caused the
EOF or that the SSH command exited for a particular reason.

The generated remote command has a source-proven `python3` prerequisite with no
interpreter fallback. The source does not prove that Python was absent or
incompatible in this run. The OpenSSH argument list also has no version probe;
the logs contain no SSH stderr or SSH version that would establish an
unsupported option. No macOS-specific root cause is source-proven.

## Hosted observation

- At source `2efb76d`, Foundation `macos-latest` and `ubuntu-latest` both
  completed successfully (761 and 763 passed, respectively; seven ignored in
  each job). The supplied universal job log identifies its runner as
  `macos-15-arm64` and records failures in the managed SSH OpenCode and Codex
  cases. This is a platform/runner difference, not proof of an OS defect.
- The log records pre-Hello stream EOF retries with
  `hello=false, active=false`, followed by the tests failing to observe a
  tracked provider start. In the OpenCode snapshot, the one-time untracked
  fallback is reported after Desktop route admission was not confirmed. With no
  `Active` handshake, the route cannot be admitted; fallback is downstream
  behavior, not the cause of the stream close.
- The same log also contains a garbled Python heredoc entered through the
  terminal PTY. That is the separately owned test transport/setup issue. The
  lifecycle operation uses a separate non-PTY SSH command (`-T` in
  `crates/remote-runtime/src/connection.rs:155-186`); the PTY text corruption
  does not establish why that child closed stdout and is not used as a causal
  explanation here.
- `EPERM` appears from cancellation in the same failure output. In source,
  `run_attempt` returns on output EOF and then `ChildOwner` teardown calls
  `killpg`; the supervisor logs the retry afterward
  (`crates/desktop-runtime/src/remote_agent_lifecycle.rs:664-800,
  349-408, 461-475`). This ordering is consistent with a cleanup error after
  the observed EOF. It does not reveal the remote command's exit status or
  explain the EOF.

## Source path and evidence gap

1. `ManagedConnection::agent_lifecycle_spec` constructs
   `ZMUX_SHELL_INTEGRATION_DIRECTORY=<nonce-root> python3 <nonce-root>/claude-lifecycle-stream.py <token>`
   and passes it to `operation_spec`
   (`crates/remote-runtime/src/connection.rs:149-205`). The operation is an
   SSH command through the existing ControlMaster, with `-F none`,
   `ControlMaster=no`, `ControlPersist=no`, `BatchMode=yes`,
   `ProxyCommand=/usr/bin/false`, `PermitLocalCommand=no`, and `-T`
   (`:155-186`). A missing `python3`, missing helper, interpreter failure, SSH
   option/config failure, authentication/master failure, or other early remote
   failure could all end before the first protocol frame. The supplied log
   distinguishes none of these.
2. `run_attempt` pipes stdin/stdout, sends stderr to `Stdio::null()`, and puts
   the SSH child in its own process group (`remote_agent_lifecycle.rs:560-602`).
   It reads stdout nonblockingly, parses only complete newline-delimited
   frames, and sets `hello_seen` only after a valid `Hello` frame passes
   version, nonce, generation, and ordering checks (`:664-783`). A zero-byte
   read sets `output_eof`; if no valid Hello was accepted, the owner returns
   `stream closed (hello=false, active=false)` (`:667-800`). EOF means the
   stdout pipe closed; it is not itself an exit status. A partial frame or no
   frame does not count as Hello.
3. The loop does not poll child status. `ChildOwner::try_wait` wraps the owned
   `Child::try_wait` and marks `reaped` on `Some(status)`, but that wrapper is
   currently test-only (`:342-347`). `Drop` first tries the owned process
   group, then polls/reaps for at most 250 ms, and transfers an unreaped child
   to the bounded reaper (`:349-408`). The exit status observed during teardown
   is not retained in the pre-Hello diagnostic. Do not infer `exit(1)`, signal
   termination, or a live child from the current message.
4. The real-child owner fixture is `SshFixture` in this module. The strongest
   existing boundary is `retries_a_real_managed_ssh_bootstrap_race_then_reports_session_loss`
   (`:1152-1262, 2758-2849`): it starts a real loopback SSH fixture, starts the
   lifecycle stream before bootstrap readiness, observes pre-Hello attempts,
   releases bootstrap, and receives a validated handshake. It tests the
   expected startup race, but not sanitized stderr classes or exit-status
   reporting. `child_owner_signals_live_group_then_skips_the_group_after_reap`
   (`:1723-1779`) covers the existing no-signal-after-reap identity guard.

## Smallest bounded diagnostic if the cause remains unknown

Add evidence only for **stdout EOF before a valid Hello**. Report a structured
status (`exit:<numeric-code>`, `signal:<numeric-signal>`, or `unknown`) and one
closed stderr class. Never log raw stderr, command arguments, host/key/control
paths, nonce/token values, or user/provider payloads. Unknown or unrecognized
text maps to `unclassified`; exact text matching must not assume that every
OpenSSH build uses the same wording.

- Use the existing owner loop and poll the piped stderr descriptor alongside
  stdout and conditional stdin readiness. Set stderr nonblocking. Give stdout
  and stderr a bounded per-iteration read quantum, alternating which ready
  stream is serviced first so a continuously noisy stderr cannot starve frame
  parsing. Keep the existing poll interval and handshake deadline.
- Retain at most a small fixed prefix (for example, 4 KiB) only while
  `hello_seen` is false, solely to classify fixed, allowlisted conditions such
  as SSH option/config failure, master/connection failure,
  authentication/host-key failure, or missing remote interpreter/helper. Emit
  only the enum. When the cap is full, continue reading and discard excess
  bytes; stopping reads at the retention cap could fill the OS pipe and block
  SSH before it exits or sends a frame. Clear the retained bytes once Hello is
  accepted. Do not wait for stderr EOF; at stdout EOF classify only bytes
  already drained, using `unclassified` if evidence is incomplete.
- If status is collected, call nonblocking `try_wait` only on this
  `ChildOwner`'s `Child`. On `Some(status)`, store only its numeric exit code or
  signal and set the same owner's `reaped` state before teardown; `Drop` must
  then continue to skip `killpg`. Never call `wait()` for diagnostics, probe or
  signal a naked PID, or signal the process group after `reaped`. On `None` or
  error, status remains unknown and the existing bounded cancellation/reaper
  path remains responsible. Reaping the leader before teardown means the group
  can no longer be signaled safely; if the owner contract requires group
  cancellation after leader exit, leave status unknown unless a non-reaping
  observation can be justified on both Darwin and Linux. This check is not
  permission to weaken the current owner/process-group safeguards.
- Keep the evidence local to this pre-Hello EOF path. Do not retain or emit it
  after Hello, on ordinary cancellation, timeout, or post-Hello loss.

At the real `SshFixture`/stream-owner boundary, add a controlled pre-Hello
remote command that writes a canary path/token and more than the retention cap
to stderr, then exits with a known code. Assert the structured code/class,
absence of the canary and all raw stderr from diagnostics, and that the child
does not stall after the capture cap. Add a Hello-then-close case proving that
no pre-Hello diagnostic is retained after Hello. Exercise signal status only
through the actual owned-child/status path and retain the existing test that a
reaped process-group ID is never signaled. Do not add a duplicate private
predicate-only test or change the separate `apps/cli/tests/ssh.rs` transport
repair.

## Proof limits and scope

- The Foundation comparison and universal failure are at the same source SHA,
  but do not isolate OpenSSH version, remote interpreter state, startup timing,
  architecture, or process-group behavior. The logs record neither child
  status nor stderr because the production owner discards stderr.
- A remote `python3` dependency is confirmed by the generated command; its
  absence or incompatibility in the failing hosted test is not confirmed. The
  source has no SSH-version guard, but no unsupported macOS/OpenSSH argument is
  demonstrated.
- Read-only source/log diagnosis only. This slice creates only this reference;
  source, tests, plan/status, index, workflows, and applications were not
  changed or run here.
