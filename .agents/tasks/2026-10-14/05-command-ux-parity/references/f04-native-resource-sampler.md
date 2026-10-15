# F04 packaged resource telemetry: first slice

`scripts/check-macos-app` runs an external sampler during its primary Launch Services
acceptance run. The sampler stops and joins before any direct/debugger diagnostic
rerun. Both successful and failed workflow artifacts include `native-resources.json`;
partial samples are atomically saved during the run. Missing telemetry never replaces
or fails the existing native correctness gate.

## Recorded evidence

- Exact bundled source SHA and named host context: macOS/kernel, hardware model,
  architecture, logical CPUs, RAM, GitHub runner architecture and image version.
- A newly launched app is identified by public `proc_pidpath` matching the exact
  bundled executable. Pre-existing matching instances are excluded. PID plus process
  start time prevents silently following a reused PID.
- At most one resource sample per second: app RSS in KiB, cumulative CPU seconds,
  interval app CPU percentage, current app descendant process count/RSS and per-PID
  measurements. First-sample CPU percentage is null, not a guessed zero.
- The private SSH server is a separate fixture process and is excluded from the app
  descendant totals. Detached/reparented children and WebKit helpers are not attributed
  by this initial PPID-tree sampler. Summed RSS can double-count shared pages and is
  not physical footprint.
- Immediately before each CLI probe, `lsof` must identify the configured control socket as
  owned by the exact app PID. Probe `status` with the bundled CLI at most once per
  second, recording elapsed milliseconds and success/unavailability. This includes
  CLI launch and socket handling; it is not input-to-display or main-loop latency.
- Each sample includes observer wall time. Sampling plus CLI work perturbs the
  workload; the one-second interval begins after the probes finish, so actual rate
  can be lower. No probe stdout/stderr, process arguments, terminal content, passwords,
  SSH keys, hostnames or socket paths are persisted in this artifact.

## Bounds and lifecycle

Probes have one-second subprocess deadlines. The loop caps at 150 samples or 155
seconds, whichever comes first; stopping interrupts its between-sample wait. Join is
bounded to five seconds. Exceptional stop timeouts leave a partial/unavailable report;
the observer never signals the app or fixture processes. Timed-out observer subprocesses
are reaped by Python. Process discovery and descendant traversal have explicit size
limits. Unavailable metrics have explicit states/reasons rather than fabricated zeros.
Finalization, document mutation and the final atomic rename share a lock so a late
worker cannot overwrite the finished report or append samples after counters finalize.
Probes, temporary-file writes and thread joining do not hold that lock. The five-second
bound applies to joining; ordinary artifact filesystem writes are best-effort synchronous
operations, not a promise to interrupt a stalled filesystem syscall.

## Acceptance boundary and next slice

This instruments the whole existing packaged journey on the workflow's `macos-15`
runner. It makes no isolated idle/output/hidden-tab/SSH-stall phase claim, establishes
no performance budget, and does not select an engine or justify higher capacity limits.
Linux can verify Python syntax and unavailable-platform reporting only; actual macOS
process/socket attribution and samples require the hosted packaged run.

Next, coordinate phase labels and controlled workloads inside the existing native
acceptance journey: idle one/eight terminals, visible output, seven hidden output tabs
with a foreground probe, and authenticated SSH backpressure with a local probe. Measure
send-once marker-to-observed-output latency and cleanup separately. Main-actor drift,
physical keystroke/display latency, browser process attribution, longer soak and proposed
numeric budgets remain open F04 work. No new production injection or fake receipt API
is introduced by this first slice.

Fresh review found stale cached socket attribution and a final-report publication
race after a worker join timeout. Both are repaired: ownership is checked for each
probe, and publication/finalization use one lock without holding it across probes
or thread join. Portable syntax/unavailable-platform/process-parser checks and a
controlled delayed-publication check pass; fresh repair review reports no blocker.
Filesystem artifact I/O remains best-effort rather than a hard real-time guarantee.
