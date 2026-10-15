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
- [S01 session persistence](references/s01-session-persistence.md) — durable topology, transactional restore, identity reservations and remaining session gates.
- [Local cloud](references/local-cloud.md) — Docker service, tunnel target, configuration isolation and verification limits.
- [T02 navigation](references/t02-navigation.md) — shared directional actions, focus history, split-close restoration and current verification gates.
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
