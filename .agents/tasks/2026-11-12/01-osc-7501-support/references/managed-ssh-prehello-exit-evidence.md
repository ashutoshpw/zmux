# Managed SSH pre-Hello EOF evidence

**Date:** 2026-11-13

**Scope:** `crates/desktop-runtime/src/remote_agent_lifecycle.rs` owner and its tests.

**Diagnosis:** [managed-ssh-prehello-exit-diagnosis.md](managed-ssh-prehello-exit-diagnosis.md)

## Implemented evidence

The lifecycle owner now pipes SSH stderr and polls it with stdout and conditional
stdin readiness. It retains at most 4,096 stderr bytes before a valid `Hello`.
Each ready stream is drained for at most four reads of 4,096 bytes per poll
iteration. Bytes beyond the retention cap are still read and discarded, so a
noisy child cannot block on a full stderr pipe. The existing stdout quantum is
unchanged. When both streams are ready, their service order alternates. Once
stderr reaches EOF, its descriptor is excluded from later polls.

Only stdout EOF before a valid `Hello`, while the handshake deadline remains
open and cancellation is not requested, produces the additional diagnostic.
It contains only `child_status=exit_code:<number>`,
`child_status=signal:<number>`, or `child_status=unknown`, plus one closed stderr
class:

- `ssh_option_or_configuration_failure`
- `ssh_connection_or_master_failure`
- `ssh_authentication_or_host_key_failure`
- `remote_interpreter_or_helper_missing`
- `unclassified`

Classification checks a bounded pre-Hello prefix against fixed, case-insensitive
markers. Unrecognized or incomplete evidence maps to `unclassified`. The raw
stderr, command arguments, host, paths, nonce, token, and user payload are never
included. A valid `Hello` clears retained bytes; subsequent stderr is drained
and discarded. EOF after `Hello`, cancellation, timeout, and other loss paths do
not emit this pre-Hello diagnostic.

Exit status comes only from `zmux_session_runtime::child::observe_child` on the
PID of this `ChildOwner`'s retained `Child`, with `job_control=false`. The
existing utility uses nonblocking `waitid` with `WNOWAIT`. `None` and errors map
to `unknown`. This does not mark the owner reaped, so `Drop` retains the child
identity for its existing process-group cleanup and bounded reaper. The
test-only `try_wait` and the guard that skips group signaling after reaping are
unchanged.

## Owner-boundary tests and evidence

`pre_hello_eof_reports_sanitized_bounded_evidence_and_hello_eof_omits_it` uses
the existing real loopback `SshFixture` and invokes the production attempt loop.
Its controlled remote shell emits a canary and `Permission denied` marker,
writes 1 MiB of additional stderr, then exits with code 37. The test requires the
allowlisted authentication/host-key class, a numeric-or-unknown status label,
no canary or raw error text, and completion before its 8-second test deadline.
It then installs a real protocol peer that writes a pre-Hello canary, waits for
the owner to drain it, sends `Hello` and `Active`, writes a second canary to
stderr, and closes. The existing post-Hello EOF message is preserved and
contains no pre-Hello labels or raw text.

`child_owner_observes_exit_and_signal_without_reaping_its_retained_child`
observes an owned shell's exit code 23 and a second owned shell's SIGTERM, and
verifies each `Child` remains retained and `reaped` remains false. The exit
case also exercises the owner's existing group-cleanup path. The existing
`child_owner_signals_live_group_then_skips_the_group_after_reap` continues to
guard the no-signal-after-reap behavior.

The test-authoring gate is satisfied at the owner boundary:

1. It protects the observable pre-Hello EOF diagnostic, stderr-cap drain, and
   post-Hello suppression behavior.
2. A regression that drops stderr, stops draining at 4 KiB, leaks raw text, or
   retains pre-Hello evidence after `Hello` fails the real-child assertions.
3. Existing bootstrap-race coverage observes retries and a later handshake but
   does not check diagnostic content, overflow draining, or Hello clearing; the
   new test failed against the previous owner message for the missing class.
4. The tests drive `run_attempt` with a real loopback SSH child and use a real
   owned child for status observation. No production API or test-only seam was
   added.

The new pre-Hello regression was first run against the previous owner behavior.
It failed at the intended assertion: the old result was only
`managed SSH lifecycle stream closed (hello=false, active=false)`, with no
allowlisted class or status evidence. After the implementation, the complete
owner module passed:

```text
RUST_TEST_THREADS=1 cargo test --locked --offline -p zmux-desktop-runtime --lib remote_agent_lifecycle::tests:: -- --nocapture
10 passed, 0 failed
```

Other completed gates:

```text
cargo fmt --check --package zmux-desktop-runtime
cargo clippy --locked --offline --package zmux-desktop-runtime --all-targets -- -D warnings
git diff --check
```

All passed. `scripts/check`, hosted gates, builds, commits, and pushes were not
run for this bounded slice.

## Limits

The 1 MiB fixture failure is synthetic evidence that stderr beyond the cap is
drained and that one known class is sanitized. Its remote exit code is known,
but the local SSH child's status may still be `unknown` at stdout EOF; the test
does not require a status to be observable in that race. The independent
owned-child test proves non-reaping status observation locally. This slice does
not establish the cause of the repeated macOS 15 EOF, prove the fixture's
synthetic failure matches that cause, or provide a macOS-hosted run. Text
classification is intentionally limited to the fixed English markers above;
all other stderr remains `unclassified`.

## Parent platform-review follow-up

Parent fixed the review P2 by compacting the actual polled descriptor prefix:
stdout is always present, live stderr is optional, and pending stdin uses the
next included slot. Stderr EOF contributes no descriptor, including no negative
placeholder, to the macOS shim. The unused sequence-gap peer import was removed.
No original hosted failure is attributed to this newly introduced review issue.
The focused owner suite and final aggregate are rerun after these corrections;
Darwin execution still requires the hosted gate.
