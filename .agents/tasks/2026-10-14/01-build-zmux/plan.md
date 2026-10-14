# Build Zmux as an independent MIT project

**Date:** 2026-10-14
**Task:** 01-build-zmux

## Objective
Build a native terminal workspace product with local automation, remote sessions,
and optional cloud/mobile clients. License original Zmux code under MIT and implement
it independently, without copying cmux GPL/BUSL code or assets.

## Context
- User selected `zmux` and `/data/PROJECTS/ashutoshpw/zmux` as the new repository.
- Repository contains planning documents on `main`; no remote is configured.
- This task currently covers planning; implementation will follow in validated phases.
- Include the behavior of cmux PR #1 through independent implementation on Zmux `main`.
- macOS-first delivery is the proposed scope; cloud and iOS follow a useful local app.
- User reports cmux hangs with remote sessions; the cause has not been diagnosed.
- Behavioral inspiration is permitted; exact cmux feature parity is not an acceptance target.

## Approach
Use a Rust core for workspace actions, session management, transport, storage, and CLI,
with a thin Swift/AppKit macOS UI. Keep engine and platform adapters behind narrow
interfaces. Validate the Rust/Swift bridge, bounded session processing, resource budgets,
and dependency licensing before expanding features. egui/eframe is deferred.

## Steps
- [ ] Capture PR #1 behavior in phase acceptance checks; deliver cohesive commits on `main`.
- [ ] M0: Establish original specification, licenses, engine/FFI feasibility, and baseline budgets.
- [ ] M1: Deliver one native terminal with isolated session work and reliable PTY cleanup.
- [ ] M2: Add workspaces, pane splits, tabs, persistence, and shared actions.
- [ ] M3: Add a versioned local socket API and thin CLI with scoped authorization.
- [ ] M4: Add SSH transport and reconnectable sessions; pass failure/resource stress gates.
- [ ] M5: Package and validate a distributable macOS MVP.
- [ ] M6: Add optional device identity, presence, and relay services when required.
- [ ] M7: Add an iOS remote-session client using the established remote contracts.

## Acceptance criteria
- [ ] Original first-party app, CLI, daemon, and service code carries MIT licensing.
- [ ] Dependency and asset notices are complete; no cmux GPL/BUSL implementation is imported.
- [ ] Local terminals, workspaces, persistence, and automation meet milestone behavior checks.
- [ ] Remote sessions survive client disconnect and enforce session ownership.
- [ ] Busy/disconnected sessions stay within documented resource limits without blocking UI.
- [ ] Rust/Swift lifecycle, cancellation, and callbacks pass native integration checks.
- [ ] The local product works without a cloud account or cloud availability.
- [ ] Native behavior is verified on macOS; portable CI alone is not native validation.
- [ ] PR #1 requirements are implemented and verified for Zmux, with its own service identities.
- [ ] Cloud/mobile milestones meet their own delivery gates before being marked complete.

## References
- [PR #1 parity and commits](references/pr-1-parity.md) — required behaviors and phased commit policy.
- [Architecture](references/architecture.md) — modules, contracts, state, and trust boundaries.
- [Session reliability](references/session-reliability.md) — worker isolation, limits, diagnostics, and stress gates.
- [Delivery roadmap](references/delivery-roadmap.md) — milestone scope, dependencies, and checks.
- [Licensing and provenance](references/licensing-provenance.md) — independent implementation rules.
- [Status](status.md) — current state, assumptions, and next action.
