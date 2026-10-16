# R03 typed input readiness consumer

2026-10-16. Original MIT Rust implementation in isolated
`ci/input-readiness-integration`; no Swift presentation, default acquisition,
activation or distribution change.

Observer version 4 requests capability `zmux.zellij.frontend.input_ready` version
1. The original native startup budget becomes one CLOCK_MONOTONIC deadline, carried
unchanged through observer and frontend descriptors. The selected dependency must
answer the real bounded capability probe before owner reservation. Earlier
observer versions retain their legacy execution and timing contracts.

The exact retained frontend receives a private one-use readiness descriptor.
Kernel PID/UID, full live Child identity, parent, namespace/socket/descriptor
inodes, modes, types, attachment ID, owner generation and nonce bind the channel.
Bounded challenge1 / input_ready2 / accepted3 framing requires reporter EOF without
trailing bytes. The CLI then rechecks cancellation, the fixed deadline and original
live owner before emitting observer InputReady. TransportConnected remains progress
only; the native Monitor separately validates the exact frontend and correlation.
Only InputReady installs a writable Session. No terminal-text inference, shell ACK,
elapsed-delay admission, input buffering or retry is used.

A terminal fact has its own typed handoff. It preserves the actual Session and
completion Saved/ACK path without installing writable state. The actual exited
Session is closed only after exit observation, so monitor/final-frame cleanup can
finish naturally. A completion arriving after Subscribe but before frontend
transport is accepted only if the exact terminal publisher re-authenticates;
transport errors, pending bytes and rejected opaque input remain failures under
the existing delivery rules.


Historical TransportConnected can race actual finite completion after the CLI
emits progress. Declared frontend parent/UID/configured image and any available
live full identity stay strict. If that frontend departed or the workload ended,
only a fresh query to the immutable original publisher can establish a pending
terminal fact. This emits no transport progress and stores no readiness frontend
or attachment. The later correlated WorkloadFinished must exactly equal that
fact before typed terminal handoff; InputReady still requires current full live
frontend/owner validation and is forbidden while a terminal fact is pending.

## Executed evidence

- Primary Monitor regression was red before repair:
  `/tmp/zmux-r03-input-readiness-monitor-red.log`. Its synthetic protocol producer
  uses actual retained peers/private namespaces and does not claim Zellij or Saved.
- Queued-transport terminal regression is deterministically red before the
  Monitor repair: `/tmp/zmux-input-readiness-monitor-terminal-red-final.log`.
  The retained fixture supervisor actually waits its direct workload Child exit7,
  then emits transport with an independent private completion responder. This is
  synthetic protocol framing with an actual wait-derived terminal result, not
  dependency readiness or native Saved proof. Green controls in
  `/tmp/zmux-input-readiness-monitor-terminal-all-green.log` cover live/departed
  frontend terminal-only admission, bad declared parent, changed live birth and
  mismatched correlated terminal event. No historical transport seeds readiness.
- Channel controls: `/tmp/zmux-input-readiness-channel-final.log`, 5 passed plus one
  explicitly invoked owned subprocess fixture. Wrong nonce, trailing bytes, a
  different retained Child, replaced socket, changed identity and fixed deadline
  fail; accepted requires actual EOF. The Ready semantic trigger is synthetic here.
- Desktop controls: `/tmp/zmux-input-readiness-desktop-final.log`, 24 passed plus
  one owned subprocess fixture, including actual PTY/S01 terminal-only ownership.
- Actual public suite: `/tmp/zmux-input-readiness-public-final-stable-r7.log`, 22 passed,
  no ignored tests. Silent actual0-output workload receives one unique line after
  typed readiness, detaches, receives a second unique line on a fresh attachment,
  retains startup count1, exits7, saves completion and physically joins. Immediate
  actual0/7 completion saves/joins without input admission. A probe-only unsupported
  configured executable refuses before owner reservation.
- Completion race red: `/tmp/zmux-input-readiness-terminal-race-repro2/5.log` retains
  exact Saved V2/reaped terminal7, physically reaped frontend SIGTERM, zero transport
  bytes/connections and no errors/loss, followed by incorrect CLI1. Focused green:
  `/tmp/zmux-input-readiness-terminal-race-green.log`; full22 green cited above.
- Namespace fixture red: `/tmp/zmux-input-readiness-public-final.log` failed an
  assumed missing-path diagnostic. Retained backend metadata independently
  recreated the renamed root as mode0775. The revised control explicitly substitutes
  a private root, requires exact immutable-root rejection, verifies its own inode
  before cleanup, and preserves the original workload. Focused green:
  `/tmp/zmux-input-readiness-root-replacement-final.log`.
- `/tmp/zmux-input-readiness-public-diagnostics-r3.log` retains a separate legacy
  observer EOF during concurrent diagnostic stress. Its immediate cause was not
  established. Bounded EOF diagnostics now finish/drain the exact CLI before failing;
  `/tmp/zmux-input-readiness-legacy-observer-diagnosis.log` and final full22 pass.
  Earlier `/tmp/zmux-input-readiness-actual-native-suite.log` likewise retains an
  unrooted generic EOF; later repaired native6 and full22 are green. These earlier
  failures are not erased or claimed diagnosed.
- A later startup failure in `/tmp/zmux-input-readiness-public-final-r6.log` is
  independently diagnosed: concurrent feature/default Cargo commands shared one
  target directory, replacing the running supervisor executable. Retained Linux
  `/proc` evidence shows its exact executable link ending `(deleted)` while the
  original server and actual shell remained alive. The full-image owner guard
  correctly refused it. Evidence:
  `/tmp/zmux-input-readiness-r6-live-process-diagnosis.json`. The foreground CLI
  Session joined; original-owner cleanup is unavailable after image replacement,
  so those original fixture owners remain retained without adopted signals or
  command replay. The earlier unrooted EOFs above are not attributed to this.
  Validation now runs with no competing Cargo commands in the same target.
  Focused stable-binary actual0/7/143 finite completion passes in
  `/tmp/zmux-input-readiness-stable-binary-bound-finite.log`.

Linux dependency binary SHA256:
`29354470f70b2965c0d273be73a17ffee42f2085cabc6dfc65cd1ea9c1e94420`.
Shared schema SHA256:
`6b82937ff88c4325c0cdcec2b016ab2fd47ef8306948f67bd97a2a360a4332e4`.
Final `scripts/check` passes362 tests in70 groups with three explicitly invoked
subprocess fixtures ignored in the normal enumeration; workspace Clippy, notices
and C ABI checks pass. Log: `/tmp/zmux-input-readiness-scripts-check-final-r6.log`.
Final strict feature Clippy: `/tmp/zmux-input-readiness-clippy-final-r6.log`.
Exact source and validation hashes are recorded in the source freeze. Consumer Darwin, packaged observer4 and native AppKit availability
remain separate gates; retained V2 run38849188824 does not establish readiness.
