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
- [references/swift-typecheck-repair.md](references/swift-typecheck-repair.md) — explicit tuple-switch return for hosted Swift6 compilation.
- [references/ghostty-fixture-repair.md](references/ghostty-fixture-repair.md) — failed ARM metadata fixture cause, source-pinned owner patch, local checks, hashes and native gate.
- [references/hosted-failures.md](references/hosted-failures.md) — exact initial Swift, Foundation and Ghostty failure evidence and rejected candidate identity.
- [references/macos-route-admission-diagnosis.md](references/macos-route-admission-diagnosis.md) — local pre-spawn deadline mismatch, exact control path and hosted proof limits.
- [references/route-start-budget-repair.md](references/route-start-budget-repair.md) — bounded Started-only CLI deadline, nested Desktop checkpoint cutoff, real-owner regressions and hosted proof limits.
- [references/ghostty-fixture-recheck.md](references/ghostty-fixture-recheck.md) — initial fixture order/ST findings and independent pinning review.
- [references/route-start-budget-recheck.md](references/route-start-budget-recheck.md) — independent deadline review and final-admission race.
- [references/route-final-admission-repair.md](references/route-final-admission-repair.md) — parent final expiry and failed-ack rollback with real-PTY red/green controls.
- [references/ghostty-fixture-final-recheck.md](references/ghostty-fixture-final-recheck.md) — final scoped source review and acceptance evidence.
- [references/route-final-admission-recheck.md](references/route-final-admission-recheck.md) — final scoped source review and acceptance evidence.
- [Protocol specification](https://superlogical.com/rex/docs/build/program-status) — revision 0.4, dated 2026-10-10.
- [Existing provider task](../../2026-11-01/01-live-agent-sidebar/plan.md) — adapter and alert guarantees to preserve.

- [references/ghostty-produced-artifact-review.md](references/ghostty-produced-artifact-review.md) — actual archive, library bytes, provenance and opt-in linkage acceptance.
- [references/macos-attachment-diagnosis.md](references/macos-attachment-diagnosis.md) — post-spawn observation RPC budget mismatch and hosted proof limits.
- [references/provider-state-budget-repair.md](references/provider-state-budget-repair.md) — bounded authoritative state RPCs, owner regressions and proof limits.
- [references/provider-state-budget-recheck.md](references/provider-state-budget-recheck.md) — independent source acceptance, test coverage gaps and nested review-pin correction.
- [references/macos-claude-fallback-diagnosis.md](references/macos-claude-fallback-diagnosis.md) — receipt-bearing Event timeout and duplicate fallback interleaving.
- [references/ghostty-linkage-compile-repair.md](references/ghostty-linkage-compile-repair.md) — hosted optional-owner compiler failure, existing-method repair and scoped compile proof.
- [references/claude-checkpoint-event-repair.md](references/claude-checkpoint-event-repair.md) — receipt-backed Event deadlines, unknown-outcome fallback policy and production-owner regressions.
- [references/claude-checkpoint-event-recheck.md](references/claude-checkpoint-event-recheck.md) — independent acceptance of deadline/queue changes and two response-classification findings.
- [references/claude-receipt-classification-repair.md](references/claude-receipt-classification-repair.md) — parent resolution, malformed/post-apply transport regressions and proof limits.
- [references/ghostty-session-linkage-diagnosis.md](references/ghostty-session-linkage-diagnosis.md) — Darwin PTY capacity, deferred-output publication barrier and prefix-accounting diagnosis.
- [references/desktop-checkpoint-fixture-repair.md](references/desktop-checkpoint-fixture-repair.md) — foreground exact-byte PTY drain, controlled incomplete-drain evidence and focused checks.
- [references/ghostty-session-linkage-repair.md](references/ghostty-session-linkage-repair.md) — deferred physical-read progress and corrected linked PTY ordering fixtures with local proof limits.
- [references/darwin-pty-repairs-recheck.md](references/darwin-pty-repairs-recheck.md) — independent source acceptance, cadence/cache consistency and abort-proof limitation.
- [references/hosted-final-87cc88d.md](references/hosted-final-87cc88d.md) — exact-source failed hosted outcomes, downloaded linkage hashes and absent app artifact.
- [references/opencode-hosted-setup-repair.md](references/opencode-hosted-setup-repair.md) — bounded OpenCode setup diagnostics, controlled preflight regression and hosted root-cause limits.
- [references/desktop-barrier-repair.md](references/desktop-barrier-repair.md) — deterministic actual-checkpoint FIFO barriers and command lifetime through public Programs selection.
- [references/desktop-barrier-recheck.md](references/desktop-barrier-recheck.md) — independent acceptance of FIFO/public Programs and engine-baseline fixture repairs.
- [references/opencode-hosted-setup-recheck.md](references/opencode-hosted-setup-recheck.md) — diagnostic-scope acceptance and child cleanup/timing coverage finding, resolved in repair evidence.
- [references/hosted-final-be8ec91.md](references/hosted-final-be8ec91.md) — accepted complete Ghostty linkage, exact-source SSH Foundation failure and pending app outcome.
- [references/opencode-publication-repair.md](references/opencode-publication-repair.md) — original-budget wait for complete terminal timing diagnostic after independent file publication.
- [references/ssh-upload-close-repair.md](references/ssh-upload-close-repair.md) — accepted cancellation versus late transport failure, deterministic owner red/green and SSH integration evidence.
- [references/final-cancellation-recheck.md](references/final-cancellation-recheck.md) — accepted diagnostic wait and already-terminal versus active-cancellation ordering finding.
- [references/browser-link-snapshot-repair.md](references/browser-link-snapshot-repair.md) — local aggregate Busy unwrap failure and bounded fixture read repair without command replay.
- [references/transfer-retirement-final-recheck.md](references/transfer-retirement-final-recheck.md) — independent acceptance of early generation-bound cancellation and terminal outcome precedence.
- [references/native-lease-diagnostic.md](references/native-lease-diagnostic.md) — aggregate-only native lease assertion, retained response diagnostic and focused pass without cause claim.
- [references/hosted-final-7a3a0a0.md](references/hosted-final-7a3a0a0.md) — Ubuntu acceptance and Darwin checkpoint socket path failures at exact source.
- [references/checkpoint-socket-path-repair.md](references/checkpoint-socket-path-repair.md) — deterministic long-TMPDIR red/green using existing checkpoint tests and short private socket roots.
- [references/hosted-final-c346051.md](references/hosted-final-c346051.md) — exact-head provider fixture Busy read failure and retained logs; native artifact not yet accepted.
- [references/provider-hook-snapshot-repair.md](references/provider-hook-snapshot-repair.md) — bounded fixture snapshot reads and preserved polling deadlines across Codex/OpenCode checks.
- [references/provider-hook-snapshot-recheck.md](references/provider-hook-snapshot-recheck.md) — independent acceptance of bounded snapshot reads and unchanged fixture contracts.
- [references/checkpoint-final-drain-fixture-repair.md](references/checkpoint-final-drain-fixture-repair.md) — actual PTY capture before child exit and unchanged pre-retirement checkpoint contract.
- [references/checkpoint-final-drain-fixture-recheck.md](references/checkpoint-final-drain-fixture-recheck.md) — independent fixture ordering acceptance and removed private-result assertion.
- [references/hosted-final-82df335.md](references/hosted-final-82df335.md) — Linux and retained linkage acceptance, macOS exec-fixture failure before app packaging.
- [references/bootstrap-exec-fixture-repair.md](references/bootstrap-exec-fixture-repair.md) — portable inert-shell kernel exec check retaining the complete bootstrap and 64 KiB assertion.
- [references/session-exit-snapshot-fixture-repair.md](references/session-exit-snapshot-fixture-repair.md) — original-deadline Busy handling in real Session reconnect exit observation.
