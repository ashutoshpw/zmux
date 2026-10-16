# Command and workspace UX parity

**Date:** 2026-10-14
**Task:** 05-command-ux-parity

## Objective
Deliver behavioral parity with the supplied cmux command inventory and close the core UX
gaps across the complete interaction lifecycle, including tab creation/reopening, browser,
sidebars, notifications, navigation and recovery.

## Context
Task 04 delivered a limited native app and identity/device service, not cmux parity.
Baseline: zmux `5642b48`; cmux `3ba653b9e5afb11d0bc578031142950d6136697e`.
Implementation started with the shared surface model and native tab lifecycle. Original roadmap and PR #1 scope remain.

## Approach
Execute the [gap-closure plan](references/gap-closure-plan.md): 26 packages, each with
cohesive commit slices and acceptance gates; all 172 audited families have a primary owner.
Start shared routing/registry/policy and the managed SSH/transfer journey.
Use shared Rust window/workspace/pane/surface actions with a thin AppKit host and public
WKWebView APIs. Implement independently under MIT; no copied GPL/BUSL code or assets.
Track CLI contracts and UI/menu/context/shortcut/palette/gesture behavior in linked ledgers.
Help-only stubs and command coverage alone do not establish product parity.
Ship cohesive validated phases on main with GitHub Actions macOS artifacts.

## Steps
- [x] Derive audit-driven work packages, dependency order and full feature-to-package coverage.
- [x] Review source and establish initial command/UX gap inventory.
- [x] Complete broader source-family audit and reconcile initial command/shortcut ledgers.
- [ ] P0: turn audited gaps into executable per-leaf/option contracts, shared registry and measured resource budgets.
- [ ] Priority journey: managed SSH plus remote file/image drop and paste; independent of cloud hosting.
- [ ] P1: topology, ⌘T tabs, ⇧⌘T/recently-closed recovery, focus history and navigation.
- [ ] P2: collapsible sidebar, attention inbox/badges and notification automation.
- [ ] P3a: embedded browser UI; P3b: full browser automation and advanced commands.
- [ ] P4: launch/open, persistence/restore, settings, themes and customization.
- [ ] P5: event automation, inspection, content tools and tmux-compatible operations.
- [ ] P6: agent messaging, hooks, hibernation and external tool integrations.
- [ ] P7: SSH/mosh/reconnect and complete independent cloud/VM command behavior.
- [ ] P8: simulator/iOS, platform/advanced commands and exhaustive parity closure.

## Acceptance criteria
- [ ] Share the next user-facing artifact only after V01 establishes full audited parity.
- [ ] Every frozen command leaf/alias/option has behavioral evidence; no unresolved gaps.
- [ ] ⌘T creates a terminal tab without splitting; tabs retain independent live sessions.
- [ ] ⇧⌘T restores closed tabs; close-other, workspace/window recovery and failure paths pass.
- [ ] Every UI entrypoint is inventoried and verified, including behaviors absent from CLI help.
- [ ] Real browser surfaces navigate and expose verified automation through shared routing.
- [ ] Sidebar toggle remains reachable; attention inbox highlights and opens exact targets.
- [ ] UI/menu/shortcut/CLI outcomes agree, including focus, errors and permissions.
- [ ] Measured resource/remote-stress budgets, native checks and downloaded artifacts pass.
- [ ] Cloud-dependent behavior passes deployed checks; hosting deferral stays explicit.

## References
- [A01 named buffers](references/a01-named-buffers.md) — private exact-text persistence and shared terminal paste, with output/capacity parity gates.
- [Shutdown fixture collision](references/s01-shutdown-fixture-collision.md) — deterministic parallel temporary-directory collision repair without runtime changes.
- [F04 controlled idle workloads](references/f04-native-idle-workloads.md) — opt-in one/eight-terminal phase ownership and bounded named-host measurements.
- [R03 native dependency packaging](references/r03-native-package.md) — opt-in ARM64 retained-package admission, unchanged executable provenance and native owner gates.
- [Browser focus fixture](references/browser-focus-native-smoke.md) — explicit document-focus setup and unchanged strict restoration checks.
- [F04 native opt-in](references/f04-native-opt-in.md) — explicit engine selection and worker-owned semantic input, with native execution gates pending.
- [H01 MFA](references/h01-mfa.md) — authenticator enrollment, login challenge and one-use recovery acceptance.
- [H01 native credentials](references/h01-native-cloud-credentials.md) — shared credential ownership, cancellation fences and accepted focused Keychain checks.
- [F04 native shutdown diagnostics](references/f04-native-shutdown-diagnostics.md) — raw Darwin child investigation and exact failing production shutdown evidence.
- [A01 current inspection](references/a01-current-inspection.md) — bounded published snapshot facts, owner evidence and remaining inspection gates.
- [Shared shutdown](references/s01-shared-shutdown.md) — one deadline, physical owner barriers and pending native lifecycle acceptance.
- [R02 local tmux](references/r02-local-tmux.md) — durable headless session ownership, verified lifecycle and remaining GUI integration.
- [R03 local zellij conformance](references/r03-local-zellij-conformance.md) — pinned MIT 0.43.0 lifecycle and passed public-API Darwin fixture; separate production native gate pending.
- [R03 headless owner design](references/r03-local-zellij-owner-design.md) — implemented foreground consumer, shared child lifecycle, peer-fenced broker, Linux evidence and remaining native gates.
- [R03 native launch consumer](references/r03-native-launch.md) — explicit foreground PTY consumer, authenticated owner receipts, shared shutdown and remaining native identity gates.
- [R03 stdin interruption repair](references/r03-zellij-eintr.md) — actual Darwin suspend failure, surgical MIT dependency patch and isolated source producer gates.
- [R03 retained dependency package](references/r03-dependency-package.md) — accepted Linux/Darwin CLI owner evidence, embedded WASM/transitive license audit and explicit future package proposal.

- [B01 browser downloads](references/b01-browser-downloads.md) — independent native resource owner, list/wait contract, safe publication and pending native acceptance.
- [B01 browser profiles](references/b01-browser-profiles.md) — explicit persistent profiles, catalog ownership, schema migration and isolation gates.
- [F04 session engine](references/f04-session-engine.md) — optional worker-owned engine, physical causal replies and remaining native/fidelity gates.
- [F04 engine evaluation](references/f04-engine-evaluation.md) — pinned permissive sources, isolated C/Rust snapshot/reflow evidence and remaining engine/native decision gates.
- [F04 candidate adapter](references/f04-adapter-candidate-design.md) — Viewport/input/effect ownership, page-allocation/history blockers, static packaging and executable fidelity/Mac workload gates.
- [F04 resource options](references/f04-resource-options.md) — native allocator/exact-prefix experiment, helper/Wasm tradeoffs and remaining resource gates.
- [F04 callback effects](references/f04-effects-slice.md) — isolated ordered BEL/reply/RESET batches, overflow/publication contracts and remaining metadata provenance gates.
- [F04 integrated owner](references/f04-integrated-owner.md) — one bounded terminal/collector, aggregate accounting and failure-fenced publication acceptance.
- [F04 metadata contract](references/f04-metadata-contract.md) — isolated provenance/capture owner, approved migration policies, local boundary evidence and production closure gates.
- [F04 kernel PTY replies](references/f04-pty-replies.md) — isolated real-child ordering, bounded reply lifetime, resize/backpressure/cancel and publication acceptance.
- [F04 worker startup](references/f04-worker-startup.md) — production worker-owned engine lifecycle and explicit startup failure before candidate linkage.
- [F04 engine artifacts](references/f04-engine-artifacts.md) — pinned static-library provenance, linked notices and fail-closed artifact prerequisites before app linkage.
- [A01 workspace metadata](references/a01-workspace-metadata.md) — scoped status/progress/log commands, aggregate bounds and native presentation gates.
- [F04 production linkage](references/f04-production-linkage.md) — explicit retained-library bridge, staged Viewport publication and native linkage gates.
- [F04 resource telemetry](references/f04-native-resource-sampler.md) — bounded external Mac measurements and remaining workload/budget gates.
- [T01 terminal input](references/t01-terminal-input.md) — legacy special-key encoding and real PTY acceptance.
- [N01 sidebar resizing](references/n01-sidebar-resizing.md) — persistent width, native drag/keyboard/accessibility controls and acceptance.
- [N01a desktop notifications](references/n01-desktop-notifications.md) — native delivery ownership, exact actions and permission acceptance gates.
- [B02a browser automation](references/b02-browser-automation.md) — asynchronous shared WebKit bridge, lifecycle fencing, owner assignments and acceptance.
- [S01 session persistence](references/s01-session-persistence.md) — durable topology, transactional restore, identity reservations and remaining session gates.
- [Local cloud](references/local-cloud.md) — Docker service, tunnel target, configuration isolation and verification limits.
- [T02 navigation](references/t02-navigation.md) — shared directional actions, focus history, split-close restoration and current verification gates.
- [T03 multiwindow design](references/t03-multiwindow-design.md) — two-window lifecycle passes independent portable checks; native execution, cross-window moves, groups and canvas remain open.
- [R01d clipboard images](references/r01-clipboard-images.md) — bounded native image preparation, exact-session receipts and upload-source lifetime; universal/native smoke passed, broader remote gates open.
- [R01c authenticated file transfer](references/r01-file-transfers.md) — private terminal channels, bounded upload/cancel, native file drop/paste and remaining image/directory gates.
- [R01b SSH workspaces](references/r01-ssh-workspaces.md) — real OpenSSH launch, remote tab lifecycle, no-focus results and remaining upload gates.
- [R01a SSH descriptor](references/r01-ssh-descriptor.md) — validated connection arguments, shared endpoint renderer and required managed-session/transfer integration.
- [F03a socket authorization](references/f03-socket-authorization.md) — local credentials/access modes, executor revocation and remaining managed-policy gates.
- [F01a command discovery](references/f01-command-registry.md) — grouped offline help, executable catalog/guide, capabilities and parser repairs.
- [F02a target routing](references/f02-target-routing.md) — implementation, red/green evidence, remaining identity gates and full-parity artifact policy.
- [Gap-closure execution plan](references/gap-closure-plan.md) — milestone order, first batch, integration, artifact gates and external dependencies.
- [Work packages](references/closure-work-packages.md) — 26 concrete owners, dependencies, commit slices and exit gates.
- [Closure tracker](references/closure-tracker.json) — all 172 audit families assigned; delivery status and evidence slots.
- [Whole-product audit](references/full-parity-audit.md) — current source comparison, principal gaps, evidence boundaries and delivery sequence.
- [CLI/shared contracts](references/audit-cli-contract.md) — grammar, targeting, authentication, output and lifecycle comparison.
- [Terminal/topology audit](references/audit-terminal-ux.md) / [rows](references/audit-terminal-ux.json) — input, rendering, tabs/windows, restore, gestures and accessibility.
- [Browser/content audit](references/audit-browser-content.md) / [rows](references/audit-browser-content.json) — browser leaves, viewers, files/search and customization.
- [Agents/remote audit](references/audit-agents-remote.md) / [rows](references/audit-agents-remote.json) — remote sessions/transfers, hooks/messages and attention/automation.
- [Cloud/platform audit](references/audit-cloud-platform.md) / [rows](references/audit-cloud-platform.json) — cloud/VM, mobile/simulator, security, distribution and PR #1.
- [Audit reconciliation](references/audit-reconciliation.json) — review routing for all 428 initial command and 171 shortcut rows.
- [Cross-cutting audit](references/audit-cross-cutting.md) / [rows](references/audit-cross-cutting.json) — Computer Use, managed policy, project pane and quit lifecycle.
- [Source catalog](references/audit-source-catalog.json) — CLI dispatch candidates and RPC identifiers; discovery is not runtime evidence.
- [Managed SSH and transfer comparison](references/ssh-transfer-contract.md) — traced source contracts, coupled delivery and acceptance gates.
- [Scrolling and attention repair](references/scroll-attention-repair.md) — corrective slice and concrete CLI parity backlog.
- [Native delivery receipt](references/native-delivery.md) — accepted artifact, checksums, screenshots and remaining gates.
- [P3 embedded browser](references/p3-embedded-browser.md) — WebKit foundation, lifecycle and open automation/profile gates.
- [P2 attention/sidebar](references/p2-attention-sidebar.md) — in-app signals, routing, badges and pending gates.
- [P1 terminal tabs](references/p1-terminal-tabs.md) — delivered contracts, evidence and remaining gaps.
- [Full UX parity](references/ux-parity.md) — interaction audit and required undo-close semantics.
- [UI action inventory](references/ui-action-inventory.json) — source-discovered shortcut actions, pending behavioral audit.
- [Source review](references/source-review.md) — findings, WebKit decision and provenance.
- [Command inventory](references/command-inventory.json) — historical 428-entry baseline; current comparison is in the audit reconciliation.
- [Phases](references/phases.md) — dependencies, ownership, implementation scope and exit gates.
- [Verification](references/verification.md) — contract fields, evidence and commit cadence.
- [Original roadmap](../01-build-zmux/references/delivery-roadmap.md) — retained M0–M7 scope.
- [PR #1 parity](../01-build-zmux/references/pr-1-parity.md) — retained auth/product requirements.
- [references/r03-package-policy-review.md](references/r03-package-policy-review.md) — exact four-package declarations, attribution evidence and independent standard-term supplementation decisions before opt-in package publication.
