# Add OSC 7501 program status alongside provider adapters

**Date:** 2026-11-12
**Task:** 01-osc-7501-support

## Objective
Implement Program Status Protocol revision 0.4 in Zmux's terminal owners, with
terminal-scoped native status and controlled alerts. Preserve existing provider
adapters and their session/turn, generation, completion, and selection guarantees.

## Context
- Implementation authorized on `main` after the read-only investigation and saved plan; use Luna workers with parent-owned acceptance.
- Baseline: `0ac5192`. Default vt100 and optional linked Ghostty have separate metadata paths.
- Existing OSC 9/777 notices are events; provider lifecycle is a separate, identity-bound owner.
- The protocol has no provider authentication, turn identity, replay cursor, or heartbeat.
- Preserve unrelated profile-lifecycle work and its modified task status.

## Approach
Keep protocol parsing, records, lifecycle and source arbitration in Rust; AppKit
renders typed snapshots and uses existing exact-terminal navigation. Reuse shared
protocol semantics across engines without introducing a competing VT parser.
OSC reports remain self-reported program status and never manufacture provider identity.
Adapter-owned terminals retain provider authority; generic OSC alerts require an
explicit single-source notification policy and never use text matching to deduplicate.

## Steps
- [x] **1. Parser capability gate:** verified authoritative parser gaps and accepted narrow VTE/vt100 and Ghostty metadata supplements; new exact-source Ghostty artifacts remain a delivery gate.
- [x] **2. Protocol owner:** add bounded revision-0.4 decoding and records, atomic rejection, replacement/inheritance/clear semantics, and lifetime events; add contract tests.
- [x] **3. Engine/session plumbing:** preserve ordered transitions and capability replies through the sole PTY writer, with staged commit/abort, exit and recovery fences; add engine and real-PTY tests.
- [x] **4. Desktop coexistence (source and focused checks):** expose terminal-scoped program records, exact selection and deterministic adapter/OSC alert ownership; add desktop, transport and alert tests.
- [x] **5. Native presentation (source complete; hosted acceptance pending):** add a Programs subgroup alongside Agents, progress and blocked reasons, safe labels and shared alert preferences; add focused packaged native acceptance.
- [ ] **6. Delivery:** document usage/limits, run required portable and applicable linked/native gates, commit cohesive validated slices, then deliver a fresh verified universal macOS development artifact.

## Acceptance criteria
- [ ] All states, fields, hierarchy, lifetime, limits and optional pre-query emission follow revision 0.4; malformed reports cannot partly mutate records or terminal output.
- [ ] Reports survive fragmentation; capability replies precede subsequent DA replies and never interleave with previously admitted input or echo report data.
- [ ] Both supported engine paths pass their applicable contracts; missing pinned-engine hooks are resolved explicitly, with no default-engine switch or hidden parser fallback.
- [ ] Provider lifecycle remains independent; OSC cannot impersonate an adapter, revive a stale session, or downgrade provider evidence.
- [ ] Program rows focus their exact existing terminal; close/replacement, SSH loss and reconciliation cannot expose live stale state or replay alerts.
- [ ] Identical reports, progress updates, resets, snapshots and suppressed sources do not create repeated sound; alert floods and transition loss have bounded, observable handling.
- [ ] Meaningful tests pass at their production owners, including real PTY round trips and packaged native visuals; no test-only production seams or duplicated parser suites.
- [ ] `scripts/check` and applicable hosted gates pass at the delivered source; final ZIP checksum, source metadata, arm64/x86_64, minimum OS and signing status are verified.

## References
- [references/design.md](references/design.md) — protocol decisions, ownership, coexistence, risks and repository anchors.
- [references/test-matrix.md](references/test-matrix.md) — observable test contracts, regressions, gaps and validation commands.
- [status.md](status.md) — planning evidence, preserved work and implementation entry gate.
- [references/implementation-handoff.md](references/implementation-handoff.md) — current ownership, accepted APIs, executable evidence and remaining integration work.
- [references/initial-slice-review.md](references/initial-slice-review.md) — independent protocol/parser/native review, findings, tested scope and source hashes.
- [references/integrated-validation.md](references/integrated-validation.md) — failed full integrated check and exact SSH argument-size proof.
- [references/integrated-review.md](references/integrated-review.md) — integrated runtime findings and proof limits before final repairs.
- [references/small-repairs.md](references/small-repairs.md) — duplicate-ID regression and Ghostty metadata-only close repair evidence.
- [references/artifact-recheck.md](references/artifact-recheck.md) — independent artifact-tool repair review and remaining hosted proof limits.
- [references/bootstrap-repair.md](references/bootstrap-repair.md) — E2BIG regression, bounded compressed transport and focused SSH/session results.
- [references/repair-recheck.md](references/repair-recheck.md) — independent bootstrap, ID, Ghostty close and dependency-notice review.
- [references/provider-epoch-repair.md](references/provider-epoch-repair.md) — bounded Session checkpoint arbitration repair, focused evidence and stopping point.
- [references/native-frame-repair.md](references/native-frame-repair.md) — delta-consumer fixture correction, bounded diagnostics and preserved ABI assertions.
- [references/provider-epoch-recheck.md](references/provider-epoch-recheck.md) — remaining unread-PTY checkpoint and reconnect ACK reservation findings.
- [references/remote-route-retirement.md](references/remote-route-retirement.md) — pending versus confirmed remote route retirement, owner regressions, focused checks, and delivery ambiguity limits.
- [references/checkpoint-pty-evidence.md](references/checkpoint-pty-evidence.md) — real-PTY checkpoint deadline, cancellation and recovery evidence.
- [references/checkpoint-drain-repair.md](references/checkpoint-drain-repair.md) — ordered PTY drain barrier and deterministic stale-snapshot regression.
- [references/remote-route-retirement.md](references/remote-route-retirement.md) — exact-generation reservation cleanup, confirmed ownership and framed SSH proof limits.
- [references/checkpoint-drain-recheck.md](references/checkpoint-drain-recheck.md) — independent drain ordering review and natural-exit publication finding.
- [references/final-drain-checkpoint-state.md](references/final-drain-checkpoint-state.md) — parent state-consistency repair and deterministic red/green PTY evidence.
- [references/remote-route-retirement-recheck.md](references/remote-route-retirement-recheck.md) — independent acceptance of scoped reservation cleanup.
- [references/graceful-close-repair.md](references/graceful-close-repair.md) — aborted-frame publication regression, bounded close polling and focused Session results.
- [references/portable-validation.md](references/portable-validation.md) — stable integrated full-check pass, source fingerprints and explicit hosted gates.
- [references/final-session-repairs-recheck.md](references/final-session-repairs-recheck.md) — independent acceptance and focused re-execution of the final two Session repairs.
- [Protocol specification](https://superlogical.com/rex/docs/build/program-status) — revision 0.4, dated 2026-10-10.
- [Existing provider task](../../2026-11-01/01-live-agent-sidebar/plan.md) — adapter and alert guarantees to preserve.
