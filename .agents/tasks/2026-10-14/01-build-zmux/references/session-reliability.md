# Session reliability and resource validation

## Purpose and evidence boundary
The user observed cmux hanging while remote sessions were active. No root cause has been
established. Rust/Swift separation is a design decision, not proof of a fix or reduced
resource use. Diagnose any reproduced Zmux freeze from bounded traces and process samples.
Do not profile, relaunch, kill, or alter the user's running cmux as part of this plan.

## Runtime requirements
- Keep PTY reads, SSH operations, terminal parsing, storage I/O and process waits off the
  native UI thread. The Rust workspace coordinator must also stay free of blocking I/O.
- Isolate ordered work per session without requiring one OS thread per session; choose
  async tasks, dedicated blocking workers, or engine threads from the actual I/O contract.
- Never hold state locks across I/O, cross-language callbacks, or native rendering calls.
  Limit critical sections and record lock-wait timing when diagnostics are enabled.
- Bound output buffers, scrollback bytes/lines, replay retention, subscription queues,
  pending commands, and in-flight connection attempts. Make configured limits observable.
- Use bounded per-session processing quanta and coalesced repaint requests so a noisy pane
  cannot starve workspace commands, keyboard input, or other terminal panes.
- Preserve terminal byte ordering. Backpressure live output rather than dropping arbitrary
  bytes that would corrupt terminal parser state. Scrollback/replay eviction follows explicit
  policy; replay gaps trigger defined resynchronization or a visible recovery error.
- Treat visual frames as replaceable snapshots, while terminal bytes and mutation results
  have different integrity guarantees. Document event loss/recovery for slow subscribers.
- Version connection attempts and invalidate stale completions. Permit only the intended
  active reader/writer per session; bound exponential reconnect backoff with jitter and
  cancel retries on explicit close. Never resend uncertain input as an automatic retry.
- On disposal, detach callbacks, cancel owned work and reap local processes according to
  the explicit close policy. Keep remote sessions only within the daemon's retention policy.
  A close command must acknowledge progress promptly without waiting on a dead connection.
- Add a local helper process only when measured failure containment needs justify it.
  It adds IPC, supervision and packaging costs; task isolation alone does not contain crashes.

## Resource measurements and limits
During M0, record the machine, macOS version, engine version, build mode, session count,
output workload, scrollback setting and run duration. Establish numerical ceilings and
regression thresholds for CPU, resident memory, pending bytes/events, input latency,
UI event-loop delay, retry rate, and session/worker cleanup time. Use stable workloads
and release builds for release comparisons; record debug measurements separately.

Record idle and active baselines early; extend them to multiple local sessions in M2 and
remote sessions in M4. Persist neither an assumed resource advantage nor arbitrary untested
numbers as a result. Enforce concrete measured budgets before passing those milestone gates.
Memory may grow until bounded buffers fill; after warmup it must stay within the agreed
ceiling, and repeated lifecycle cycles must not accumulate workers, descriptors or processes.

## Stress matrix
These are planned checks. Each needs a workload, numerical threshold, duration, result,
and build identifier in the implementation task before being counted as passed.

| Scenario | Required behavior and observations | Gate |
| --- | --- | --- |
| Idle app and idle connected terminals | Repaint/polling rate settles; CPU and memory meet budgets | M0/M1 |
| Continuous output and long scrollback | Parsing stays ordered; buffers plateau within limits; UI responds | M1/M2 |
| Several busy panes plus one interactive pane | Focus, typing, resize and workspace changes stay responsive | M2 |
| CLI/event subscriber stops reading | Bounded event queues and explicit recovery; commands remain usable | M3 |
| Remote network stalls/disconnects during output | Local UI remains usable; retries/queues bounded; remote PTY survives as configured | M4 |
| Repeated SSH failure and reconnect | One intended active transport per session; stale completions ignored; retry limits respected | M4 |
| Sleep/wake or network switching | Connection state resolves predictably; no duplicate workers or unsafe input replay | M4 |
| Pane closes during connect/read/resize | No stale callbacks, dangling handles, blocking main-thread teardown or leaked ownership | M2/M4 |
| Multiple remote sessions over a planned soak | Resource trends plateau; healthy sessions remain usable when another fails | M4 |
| Daemon restart or replay retention exceeded | Visible, explicit recovery; no false claim of restored state or silent input execution | M4 |
| Release artifact repeats representative workloads | No unexplained regressions from established budgets | M5 |

## Diagnostics
Expose per-session connection state, generation, queued bytes, replay window, parse/update
timings, reconnect attempts and active-worker counts. Capture UI responsiveness and process
CPU/memory alongside those counters. Diagnostics should identify a saturated queue, blocked
executor, render overload, or orphaned task without logging terminal content or credentials.
Failure reports must distinguish what was observed from suspected causes and include the
build/machine/workload details needed to reproduce the issue.

## Native bridge checks
Validate callbacks arriving after cancellation, repeated create/dispose, invalid handles,
buffer ownership/release, error propagation and concurrent event delivery. Confirm that
callbacks reach AppKit on the main thread and core calls do not wait on session I/O.
Surface Rust panics as contained failures under the chosen FFI policy, never unwinding into
Swift; require explicit session recovery if mutable engine state can no longer be trusted.
Portable Rust checks alone do not establish native FFI, rendering or UI responsiveness.
