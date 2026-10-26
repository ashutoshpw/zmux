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

## First Darwin execution receipt

macOS Actions38751027370 at700648dae2f77bd4ad5c156c05b059a81807a461 compiled both
architectures and reached packaged smoke, which failed on sidebar pointer focus.
Its downloaded native-resources.json names macOS15.7.9 arm64 VirtualMac2,1, three
logical CPUs and 7GiB RAM. In the short2.55-second primary run, exact-app attribution
produced two resource samples (53,952 and57,168KiB RSS), one successful bundled status
probe (24.408ms including process launch), and one explicit socket-owner-unverified
skip. The sampler finalized before the separate direct diagnostic rerun.

These values establish that Darwin process lookup, successful socket attribution,
probe execution, unavailable states and artifact publication executed. Two startup
samples from a failed journey establish no idle cost, percentile, sustained-output
budget, remote-stall stability or comparative engine result.

## Complete packaged-journey receipt

macOS 38759082621 at 948e92282ddc84a30261b8a9628c88027ce504e3 passed Rust/ABI,
universal compilation and the required packaged smoke flags. The downloaded
build, smoke and resource documents name that exact SHA, and the development
archive's SHA-256 matches its published checksum. Both architectures compiled;
execution was on macOS 15.7.9 arm64 VirtualMac2,1. The artifact is ad-hoc signed
and not notarized.

The sampler finalized after 22.366 seconds with 19 observations: one explicit
startup app-not-found state, 18 available resource samples and 18 successful CLI
status probes. App RSS ranged from 48.09 to 106.58 MiB, current-PPID-tree summed
RSS from 50.73 to 129.14 MiB, and reported interval app CPU from 6.61 to 48.19
percent of one logical CPU. Bundled CLI process launch plus status round trip
ranged from 11.297 to 143.325 ms, with a sample median of 23.868 ms.

These are whole-journey observations with observer work and shared-page counting,
not phase-isolated costs, percentile guarantees, performance budgets, physical
footprint or input-to-display latency. Separately owned sshd and detached helpers
remain outside current PPID-tree attribution. The native report's
`window_capture_status` was 1; the archived PNGs do not establish successful OS
window capture. This receipt establishes this instrumentation subset and does not
close full F04 performance or comparative-engine acceptance.
