# Implementation and delivery evidence

## Behavioral test owners
- Desktop runtime owns applying actions to real PTYs, preserving hidden workspace sessions,
  start/close race cleanup, session limits and input/resize routing. Existing model and PTY
  tests cannot verify their actual integration. No test-only production seam is needed.
- Terminal adapter owns styled grid-run export and Unicode column positions for native drawing.
- Native packaged smoke check owns Swift/C linkage, AppKit view construction, rendered output,
  keyboard input routing, kernel resize and bundle startup. Portable tests cannot prove these.

## Verification
### Rust desktop integration
- `scripts/check`: 22 Rust behavioral tests, C ABI checks, formatting, Clippy and notice inventory pass locally.
- Public desktop tests route real shell input/resize, preserve hidden sessions, reap closed panes,
  reject excess terminals without model mutation and handle close during startup.
- Session launch happens off the action coordinator; closed launch results are canceled/reaped.
- Bound active/starting/retiring processes and queue capacity. No UI-thread process joins.
- Export styled terminal runs with ANSI colors, wide Unicode positions and terminal keyboard modes.
- Swift/AppKit compilation and packaged native smoke evidence are pending the hosted build.

## Remaining gates
Development build is ad-hoc signed, not notarized or release-signed. Interactive IME,
accessibility, selection, performance budgets, remote reconnection, cloud and mobile parity
need their own checks; successful packaging alone is not proof of those behaviors.

### First hosted macOS check
- Run `38627613214` at `0cce9ae` failed the existing output-flood cancellation regression:
  `output_flood_does_not_prevent_cancel_and_leader_reaping` timed out during shutdown.
- Linux foundation checks passed at the same SHA; native app compilation had not run yet.
- Repair: signal the process group, then close both PTY master descriptors before waiting
  for child reaping on worker destruction. Preserve the original three-second regression deadline.
- Hosted revalidation is required; no artifact is claimed from the failed run.

### Hosted shutdown follow-up
- Run `38627893169` at `e229f92` exposed EPERM on repeated process-group signaling during
  cleanup, leaving unreaped leaders detectable by the desktop lifecycle tests.
- Track successful signaling, never signal the same group twice, and poll/reap already-exited
  leaders. Treat EPERM as benign only when the owned child is independently observed exited.
- Explicit worker cleanup reports errors through terminal state, rather than only a Drop log.

### Native compiler gate
- Foundation jobs on Linux/macOS and native workflow Rust checks passed at `46e04a7`.
- Both Rust macOS targets compiled; Swift failed because the macOS 15 SDK's legacy
  NSTextInputClient protocol is nonisolated while the view is MainActor-isolated.
- Use the compiler-supported preconcurrency protocol conformance and retain MainActor
  isolation/runtime preconditions. Strict concurrency and warnings-as-errors remain enabled.

### Packaged startup gate
- Run `38628680714` at `443fde8` passed Rust validation, universal Swift/Rust compilation,
  architecture verification and bundle signing. Launch Services returned without a smoke
  report; stdout/stderr were empty. No downloadable app artifact was published.
- Preserve the Launch Services acceptance gate and add a diagnostic-only direct packaged
  executable run plus macOS crash reports when that gate fails. Local `scripts/check`
  and Python syntax validation pass; native startup remains unverified.

### Foreign-host SIGPIPE repair
- Run `38629209149` directly observed packaged executable exit `-13` (SIGPIPE).
- Extended the independent C caller to retain SIGPIPE's default disposition, exit a real
  desktop shell, then destroy its context. `scripts/check-native-bridge` failed before the
  fix with SIGPIPE on Linux, reproducing the foreign-host failure rather than mocking it.
- The wake channel uses `std::os::unix::net::UnixStream`. Linux writes suppress SIGPIPE;
  run `38629863830` at `aded543` demonstrated Darwin still needs explicit `SO_NOSIGPIPE`.
- Configure that option through the safe, permissively licensed `socket2` API on macOS.
  Preserve the embedding host's process-wide signal disposition. Closed wake readers still
  return BrokenPipe, benign for an already-stopped worker. Native revalidation pending.

### First successful packaged run
- Run `38630171775` at `078313f915c233ea6666fee6204e3720842912b0` passed universal build,
  signing and all native smoke conditions. Foundation run `38630171986` also passed.
- Artifact `11925702949` was downloaded: ZIP integrity/checksum, bundled notices, executable,
  dylib, universal headers, source metadata and smoke receipt agree.
- Native bitmap visibly renders terminal output and ANSI red; composited AppKit buttons
  were blank in view caching. Capture with effective appearance and add a WindowServer
  image when runner screen permissions permit, keeping the behavioral gates unchanged.
