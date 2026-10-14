# Command and workspace UX parity

**Date:** 2026-10-14
**Task:** 05-command-ux-parity

## Objective
Deliver behavioral parity with the supplied cmux command inventory and close the core UX
gaps: ⌘T terminal tabs, embedded browser, collapsible sidebar and attention notifications.

## Context
Task 04 delivered a limited native app and identity/device service, not cmux parity.
Baseline: zmux `5642b48`; cmux `3ba653b9e5afb11d0bc578031142950d6136697e`.
This turn creates the implementation plan only. Original roadmap and PR #1 scope remain.

## Approach
Use shared Rust window/workspace/pane/surface actions with a thin AppKit host and public
WKWebView APIs. Implement independently under MIT; no copied GPL/BUSL code or assets.
Track every command, alias, option and observable result. Help-only stubs do not count.
Ship cohesive validated phases on main with GitHub Actions macOS artifacts.

## Steps
- [x] Review source and establish initial command/UX gap inventory.
- [ ] P0: expand all nested contracts, target/auth rules and measured resource budgets.
- [ ] P1: window/pane/surface model, ⌘T terminal tabs and navigation commands.
- [ ] P2: collapsible sidebar, attention inbox/badges and notification automation.
- [ ] P3a: embedded browser UI; P3b: full browser automation and advanced commands.
- [ ] P4: launch/open, persistence/restore, settings, themes and customization.
- [ ] P5: event automation, inspection, content tools and tmux-compatible operations.
- [ ] P6: agent messaging, hooks, hibernation and external tool integrations.
- [ ] P7: SSH/mosh/reconnect and complete independent cloud/VM command behavior.
- [ ] P8: simulator/iOS, platform/advanced commands and exhaustive parity closure.

## Acceptance criteria
- [ ] Every frozen command leaf/alias/option has behavioral evidence; no unresolved gaps.
- [ ] ⌘T creates a terminal tab without splitting; tabs retain independent live sessions.
- [ ] Real browser surfaces navigate and expose verified automation through shared routing.
- [ ] Sidebar toggle remains reachable; attention inbox highlights and opens exact targets.
- [ ] UI/menu/shortcut/CLI outcomes agree, including focus, errors and permissions.
- [ ] Measured resource/remote-stress budgets, native checks and downloaded artifacts pass.
- [ ] Cloud-dependent behavior passes deployed checks; hosting deferral stays explicit.

## References
- [Source review](references/source-review.md) — findings, WebKit decision and provenance.
- [Command inventory](references/command-inventory.json) — 428 initial tracked entries; nested audit remains.
- [Phases](references/phases.md) — dependencies, ownership, implementation scope and exit gates.
- [Verification](references/verification.md) — contract fields, evidence and commit cadence.
- [Original roadmap](../01-build-zmux/references/delivery-roadmap.md) — retained M0–M7 scope.
- [PR #1 parity](../01-build-zmux/references/pr-1-parity.md) — retained auth/product requirements.
