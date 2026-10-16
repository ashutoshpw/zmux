# Terminal CPU and Claude notification investigation

**Date:** 2026-10-16
**Task:** 01-terminal-cpu-audit

## Objective
Explain the reported CPU difference and missing background Claude completion
notifications. Implement the phased repair with measured costs distinguished from
source-based hypotheses. Implementation is authorized, including local and SSH
notification integration, cohesive commits and tested macOS Actions artifacts.

## Context
Screenshot: main-app CPU 31.5% vs 10.9%, RAM 83 MiB vs 245 MiB.
zmux uses less RAM in every shown scope; workloads differ. Investigate the actual
AppKit/Rust implementation and preserve unrelated profile-lifecycle work.
User clarification: mostly idle, including without Claude; only one active Claude
session in zmux when the screenshot was captured. Missed completion was local.
Active-screen amplification is reproduced but does not establish this idle CPU cause.

## Steps
- [x] Trace terminal parsing, publication, bridge transport and native rendering.
- [x] Inspect reference rendering design without copying implementation.
- [x] Run bounded measurements supported by this Linux host; inspect retained
      native measurements and disclose their limits.
- [x] Record findings and prioritize fixes and macOS profiling needs.
- [x] Trace plain Claude launch, provider Stop events, attention and OS delivery.
- [x] Prepare a concrete phased implementation and validation plan for both issues.

## Acceptance criteria
- [x] Findings have source pointers and distinguish observations from attribution.
- [x] Explain why Rust does not itself guarantee lower CPU.
- [x] No claim to identify the physical Mac hot stack without a matching profile.

## Delivery phases
1. Profile current-build idle CPU first; identify and repair its measured owner.
2. Connect ordinary local Claude lifecycle events to shared attention.
3. Implement managed SSH completion ingress in this same session, with scoped routing.
4. Cache native text metrics and redraw only damaged rows/changed chrome.
5. Replace full-frame transport with versioned per-session deltas and bounded wakeups.
6. Verify idle CPU plus local and SSH notifications on macOS, then publish the
   tested Actions artifact. SSH integration must not be deferred to another session.

## Implementation gates
- [ ] Current-build native idle stack capture and measured owner repair.
- [ ] Plain local Claude and authenticated SSH completion attention.
- [ ] Native rendering/transport reductions with correctness preserved.
- [ ] Scoped checks, phased commits, native Actions validation and artifact handoff.

## References
- [Findings](references/findings.md) — source trace, executed backend probe and limits.
- [Implementation phases](references/implementation-phases.md) — owners, changes,
  commit boundaries and behavioral/native acceptance for each phase.
- [Native presentation repair](references/native-frame-delivery.md) — delta ABI,
  repaint ownership, bounded caches and diagnostic throughput comparison.
