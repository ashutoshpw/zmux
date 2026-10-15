# Kernel PTY reply feasibility

Original isolated MIT acceptance of the frozen single terminal owner with an
actual PTY child. Read [the contract](PTY_CONTRACT.md), [provenance](NOTICE.md) and
[design/evidence](../../.agents/tasks/2026-10-14/05-command-ux-parity/references/f04-pty-replies.md).
No application engine, native UI or production input path changes.

```sh
python3 -B tools/terminal-pty-feasibility/run.py --output /tmp/zmux-pty-results
# Explicit, labeled local content-addressed dependency-cache reuse:
python3 -B tools/terminal-pty-feasibility/run.py \
  --dependency-cache /tmp/task-owned-zig-cache --output /tmp/zmux-pty-cached-results
```

The output directory must be new. Requirements match the accepted experiment:
Rust 1.95, Python 3.12+, C compiler, pinned Zig/source archive network availability;
Mac runs need Xcode command-line tools. Linux x86_64/macOS arm64/x86_64 build and
execute only their native host architecture. There is no universal-slice claim.

The runner imports the shared download/build/containment helpers, verifies
all 31 base hashes against `base-lock.json` (28 accepted inputs unchanged, plus
the revised runner, shared lifecycle helper and regression tests), applies the two accepted patches to
fresh checksum-verified source, and records new input/command/log/library hashes.
Each process group has a bounded deadline and at most 1 MiB retained logs. The
fixture's requested three-CPU-second/512-MiB AS/no-core limits are reported as
configured/unavailable; these are containment, not measured product budgets.

One raw no-echo kernel PTY and one exec'd Python child exercise literal cursor/
status replies between real PRE and POST input, metadata and kernel resize,
actual EAGAIN backpressure, explicit Busy/retry, a 250-ms stalled-queue deadline,
child-confirmed three-byte progress then peer close/cancel, real ioctl error on
an owned closed descriptor and engine export failure. Child/FD cleanup is checked
before destruction. No terminal parser exists outside the accepted Ghostty owner.
A separate two-second subprocess sets `RLIMIT_NOFILE` and exhausts real descriptors
after report-pipe creation, verifying prompt construction failure and physical
closure of all acquired descriptors. Report preservation never reads a pipe whose
nonblocking setup did not complete.
Direct-child cleanup cases observe real exited/unreaped state at the signal
boundary and require reaping without signalling; a separate actual ECHILD case
checks precise operation/errno diagnostics and refuses stale-PID signals. Linux
coverage of unnecessary signalling does not reproduce Darwin's errno behavior.
Cleanup closes the owned master before bounded reaping waits and retains report
capture. A stalled real child guards physical close-before-wait ordering; Linux
does not reproduce Darwin's output-blocked exit behavior merely by passing it.

The coordinator seals a validated frame/metadata result and defers exact commit
until its replies have entered the kernel. Kernel acceptance differs from child
consumption; the latter is separately proven by the child. On partial delivery
failure, already accepted bytes cannot be undone; the unpublished candidate stays
hidden and the owner quarantines without replay. Resize errors likewise make no
rollback claim. Native shell/job control, key events and production integration
remain separate gates.

Two temporary original-coordinator mutations duplicate a reply byte or publish a
partly delivered candidate. Both must fail their named independent child/order or
last-good assertion with exit 101; a complete receipt includes those intended reds.

Storage remains separately stated: 24,576 total reserved reply/input payload bytes
across unused Probe, collector, both publication slots and transport queue, already
included in fixed C/slot/inline charges. Observed queue high-water is diagnostic,
not a capacity proof. Child buffers/threads, kernel queues and total RSS are outside
that ledger. A deterministic syscall budget exercises partial progress; only an
observed kernel short-write counter establishes actual short-return behavior.
