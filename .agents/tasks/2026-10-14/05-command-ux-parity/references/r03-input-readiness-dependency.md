# R03 optional dependency input-readiness reporter

2026-10-16. Authorized implementation is isolated on
`ci/input-readiness-dependency`; no push, package acquisition, core/runtime change
or default/native activation is authorized by this slice.

The parent-reviewed [readiness design](r03-input-readiness.md) remains the behavior
contract. The exact wire proposal is
`tools/local-zellij-feasibility/input-readiness-protocol.json`. Parent source and
schema review precedes any dependency build. V6 source/binary/receipts stay
immutable; all extraction, patches and build evidence use a fresh output root.

## Proposed source footprint

- New original MIT `zellij-client/src/input_readiness.rs`: strict bounded optional
  descriptor, two-condition reporter, cancellation and authenticated challenge /
  input_ready / accepted channel, with real owned-child/socket controls.
- `zellij-client/src/lib.rs`: optional reporter initialization, actual parser
  closure transition, first successful typed Render flush, and joined lifetime.
- `src/main.rs`: capture/remove the descriptor environment as the first main
  operation, then the sole capability probe before logger/config/cache. Pass the
  owned optional path explicitly through `src/commands.rs` into `start_client`;
  no singleton/global descriptor or environment reaches spawned processes.
  Readiness mode requires an existing explicit Attach and rejects creation,
  background, server, web and reconnect paths. Ordinary behavior stays intact.
- No upstream manifest/lock changes. Existing std/serde/serde_json/uuid/nix/libc
  suffice. Original small Unix FD-ownership and Darwin LOCAL_PEERPID wrappers
  require source review; interprocess1.2.1 cannot obtain Darwin peer PID.
- New patch inventory and opt-in producer mode, real reporter/actual-frontend
  fixtures, source/patch/protocol hashes and README/provenance instructions under
  `tools/local-zellij-feasibility/`. Preserve existing EINTR and owner gates.

The frontend authenticates its live direct parent using kernel socket PID/UID,
current getppid, nonce and pinned private inode context. The integrating CLI must
independently authenticate its retained frontend's full immutable identity. This
does not invent a dependency-side full parent image/start API or treat supplied
PID fields as kernel credentials.

## Validation sequence

1. Freeze exact schema/capability and obtain parent review of source footprint.
2. Record honest existing capability absence and add the real new reporter
   controls; do not fabricate a missing-symbol or compile-failure red. Native
   TransportConnected/Ready admission regression belongs to the integrating owner.
3. Exercise both conditions, real peer/nonce/ACK mismatch, deadline/cancel,
   duplicate/full queue and partial/backpressured control I/O with owned children.
4. Build the explicit new producer and verify its capability is side-effect-free;
   preserve lock/EINTR/provenance and run actual frontend first-line delivery.
5. Send immutable reviewed source/evidence for integration with observer v4 after
   the independent V2 observer-v3 cohort freezes. No runtime files belong here.

## Pre-build source freeze

The pre-build review manifest is
`/tmp/zmux-input-readiness-source-freeze-20261016-v2/review/manifest.json`, SHA-256
`14da67a48efa93d0525986bd7977a46e6c76fb1a2fd490048fcd9866af854613`.
The pre-build inventory SHA-256 is
`87cbeb1113ef8eee722f7fcb82ba39d01c17a01cf7035e994abebd9339af70f0`;
schema remains `6b82937ff88c4325c0cdcec2b016ab2fd47ef8306948f67bd97a2a360a4332e4`.
At this freeze, the module, patch, producer, protocol, real owned-child tests and
silent frontend fixture had only passed `rustfmt` and Python byte compilation.
The parent reviewed the production patch/reporter and authorized one named build
and actual silent-workload acceptance before any dependency compilation. The immutable v6 executable remains
`b1c2c13d14e088d2c43601e078d8de5abb5e940a3e75aacb490b5aa472aaba59`.

The producer preserves EINTR red/green and non-EINTR controls, then only in the
explicit readiness mode applies the inventoried patch/copy and runs the new
reporter controls before the optional complete executable build. A real capability
probe uses an empty isolated HOME/config/cache/runtime directory and requires both
exact stdout and no filesystem entries. Package rebuilding rejects this mode.

The new owner fixture authenticates the actual retained frontend through its
real parent-created listener, handles queries independently, awaits accepted3/EOF,
then writes one unique line once to a silent real workload. A second attachment
uses a fresh namespace/nonce and a different line; private exact stdin records
and startup count one protect reattach behavior. Cleanup signals only owned
children and uses explicit isolated session control for the retained backend.

## Linux dependency acceptance

The named producer output is
`/data/PROJECTS/ashutoshpw/zmux/target/agent-validation/zellij-input-readiness-v1-20261016`.
Its passed receipt SHA-256 is
`64524b84c09b2a322686eb9bba58aa3f336ef3e43b5b960039d5f56b7733abfc`;
`artifacts/zellij` SHA-256 is
`29354470f70b2965c0d273be73a17ffee42f2085cabc6dfc65cd1ea9c1e94420`.
The upstream lock remains unchanged. Existing real EINTR red/green and non-EINTR
controls passed, followed by seven real reporter tests; one ignored test is the
reexec child fixture, deliberately invoked by its retained parent tests. The full
binary and exact capability/empty isolated HOME probe passed. The controls cover
fragmented/stalled input and duplicate-render suppression; outgoing socket
backpressure was not independently forced.

The actual owner passed at
`/data/PROJECTS/ashutoshpw/zmux/target/agent-validation/zellij-input-readiness-owner-v2-20261016/receipt.json`,
SHA-256 `6d97492d86be2c5969dba77642cfd456e2d290bee55c232a18e21c32964ef870`.
Both genuine frontends matched kernel peer PID/UID and retained start/boot/image,
completed accepted3/EOF while alive, then each received one unique line once.
Private workload records contain exactly those two complete lines, startup count
stayed one, both frontend exit codes are zero, and isolated session control ended
the retained backend/workload. No terminal marker establishes readiness.

The first actual-owner attempt is retained as failed: compact KDL in the fixture
was invalid, before backend creation or any readiness/input event. Repairing only
the fixture's KDL and independent provenance check required no dependency rebuild.
The passed owner binds the original producer inventory, verifies its exact four
compiled file hashes and all build inputs, and records the original and repaired
fixture hashes separately. Current inventory SHA-256 is
`49e0a0f1d8cdaec3b9c3e7e50d552b40c2694c2f3e0323362b535b27f9748cad`.

The retained v6 executable honestly rejects the new capability argument (exit2,
no timeout), recorded in `/tmp/zmux-input-readiness-old-capability-20261016.json`;
this is capability absence, not a fabricated behavioral red. The immutability
check compares all 1334 archived v6 source files, retaining only its known original
EINTR patch plus its added fixture, and verifies its original archive/binary hashes:
`/tmp/zmux-input-readiness-v6-immutability-20261016.json`.

Reporter failure notification is best effort: upstream can buffer Error while
loading or reject the nonblocking notification when its bounded queue is full.
The reporter closes on failure; the integrating consumer's fixed deadline,
cancellation and retained-child cleanup remain mandatory. This slice does not
claim immediate frontend exit on every reporter failure.

Darwin execution, full native/CLI observer4 admission, completion-before-Ready,
package redistribution and default activation remain separate gates. No core,
runtime, package acquisition or running-user-app behavior changed in this slice.

Canonical runtime_base resolves /tmp (including Darwin /private/tmp) with sticky
ownership checks. All deadlines use actual CLOCK_MONOTONIC milliseconds, never
Rust Instant epoch. This updates the original three-file footprint to four
upstream files to carry captured environment explicitly through commands.rs.

## Repository check and proposed Darwin lane

`scripts/check` passed on this dependency branch: 343 Rust tests passed, one
existing test ignored, strict Clippy, notice inventory and native C ABI checks
passed. Receipt:
`/tmp/zmux-input-readiness-scripts-check-v2-20261016/receipt.json`, SHA-256
`cdd92722847d3fbe5b8e2aafccaf9791d2aaa09c73351c4dd18d493cd147f5a1`.
The first invocation stopped at pre-existing formatting in the completed-attach
assertion. Existing main fix `a7f06dd` was cherry-picked as `e2c0559` before the
successful complete invocation; the Windows stress repair was not needed.

The parent reviewed the proposed Darwin ARM64 workflow. It requires the exact
current source inventory SHA-256 at manual dispatch, builds a fresh optional
dependency, then runs the same real reporter controls and silent foreground
fixture. It retains sources and host evidence without executable/package upload,
application build or activation. The workflow has not run at this checkpoint;
macOS 14 runtime compatibility and native observer4 remain separate claims.

The proposed registered dispatcher route adds `input_readiness=false` and an exact
reviewed hash to `local-zellij-patched-owner.yml`. A true selection calls the new
workflow through `workflow_call`; its legacy matrix is unchanged when false.
Readiness plus package publication is an explicit preflight error. This allows a
feature-branch diagnostic without first registering the new filename on main.

## Darwin dependency acceptance

The reviewed registered route ran once at exact source commit
`d36bc912c0889436c055a7daeb21bead1156b65f`:
[run 38849682804](https://github.com/ashutoshpw/zmux/actions/runs/38849682804).
Only the Darwin readiness job executed; the legacy owner/package job was skipped.
The exact reviewed inventory remained
`49e0a0f1d8cdaec3b9c3e7e50d552b40c2694c2f3e0323362b535b27f9748cad`.

On macOS 15.7.9 ARM64 the fresh source producer, existing EINTR/non-EINTR controls,
seven reporter tests, exact capability/empty HOME probe and actual silent workload
all passed. Both genuine attachments completed accepted3/EOF with the original
frontend alive before one unique line was submitted once. Exact private records
contained two complete lines; startup count stayed one. Both frontend exits were
zero, and the backend/workload identities disappeared through isolated cleanup.
The real peer controls exercise Darwin's public LOCAL_PEERPID path.

The retained source/evidence artifact ZIP SHA-256 is
`d04c0bc7fb84f2a18c7c3f4da7414e4d353b5e804976d7e8c73361a41d7abfe6`,
matching GitHub artifact `12024539448`. Review verified all 18 retained repository
sources against the exact Git commit, all four compiled upstream files against
the inventory, producer/owner log hashes and semantic receipt fields. Review:
`/tmp/zmux-input-readiness-darwin-38849682804-review.json`, SHA-256
`f3feceba9960f7cbf98dd25d56b70179aaadd4c0b4a26db5922888e96ea7e274`.
Producer receipt SHA-256:
`3223b5785ac2cd318160e5909a8335d1dba50866725318be922c04ac9687da67`.
Actual owner receipt SHA-256:
`5e8b36934e7f31d76d8ccee42fd8b0a6a2504ace99af8fb30dab6e89a95a577a`.

The executed Darwin binary hash recorded by both receipts is
`b69867c654c5c0d78304725acf57610eda918a2be4c4ec684613b52a255844b6`.
Executable bytes were deliberately not uploaded. This is dependency control and
actual frontend acceptance, not a retained distributable package, native
observer4/completion-before-Ready acceptance, or macOS 14 runtime proof. A native
Darwin consumer needs the same fresh producer in its own reviewed job or an
explicitly accepted package; it cannot treat this source-only artifact as a
binary handoff. The seven-day artifact retention remains a bounded evidence gate.
