# Delivery roadmap

Every milestone should receive a scoped task record when implementation begins, linked
back to this plan. Record actual commands and results there. The checks below are planned
behaviors, not executed tests. Do not introduce CI jobs for modules that do not exist yet.

The PR #1 parity matrix in [pr-1-parity.md](pr-1-parity.md) is required scope.
Cloud remains optional for end users, but its implementation is part of the full project plan.
Commit each validated cohesive slice, update status, and land completed work on `main`.

## M0 — Foundation and feasibility
Dependencies: none.
- Write original product requirements and a small observable-behavior specification.
- Confirm MIT copyright holder, dependency policy, and minimum supported macOS version.
- Add MIT license, dependency/provenance inventory, contribution guidance, and module rules.
- Evaluate a pinned upstream terminal engine (Ghostty is a candidate, not a selection).
  Verify its actual dependency/asset licenses and supported embedding/build interfaces.
- Prototype upstream engine embedding without reading/copying cmux integration code.
- Establish a macOS build machine and minimal build instructions; check name availability.
Exit: native prototype displays an interactive shell with keyboard input, resize, Unicode,
and clean shutdown; engine decision and license inventory have concrete evidence.
If embedding is unsuitable, evaluate another permissive engine before expanding the app.

## M1 — One working terminal
Dependencies: M0.
- Create app shell, engine adapter, PTY ownership, focus and clipboard behavior.
- Support shell selection, working directory, exit status, and explicit error presentation.
- Respect platform permissions and avoid logging terminal content.
Exit: interactive shell, resize, copy/paste, IME and Unicode work on a real macOS build;
repeated open/close leaves no app-owned orphan PTYs/processes or obvious resource growth.

## M2 — Workspace model and persistence
Dependencies: M1.
- Implement split tree, tabs, workspaces, focus traversal, rename and close policies.
- Route UI and keyboard commands through shared actions.
- Add versioned snapshot persistence and safe restoration.
Exit: model checks cover invalid actions, focus after closure, split-tree invariants and
out-of-order effect completion; native checks cover layout, accessibility, keyboard-only
navigation and restart. Corrupt/older snapshots recover without silently losing the original.

## M3 — Local automation and CLI
Dependencies: M2; protocol design can begin after M0.
- Define versioned schema and original contract fixtures.
- Implement local socket authentication, bounded parsing, dispatch, and subscriptions.
- Add CLI list/create/focus/split/close and explicit terminal-input operations.
- Add scoped agent notifications without giving notification payloads execution authority.
Exit: UI and CLI produce equivalent state for equivalent actions; integration checks cover
unauthorized peers, oversized/invalid input, stale IDs, concurrent requests and disconnects.
Document automation's same-user trust boundary and destructive command semantics.

## M4 — SSH remote sessions
Dependencies: M3; daemon prototyping may follow protocol stabilization.
- Implement remote PTY daemon, framed transport, session IDs, bounded replay and cleanup.
- Use SSH host verification and explicit agent installation/version negotiation.
- Implement reconnect, resize, exit reporting and predictable retention limits.
Exit: a two-host check proves commands execute on the remote host; connection loss preserves
the remote process, reconnect restores available output, and replay gaps are reported.
Unauthorized session access fails; failed SSH never executes commands locally. Validate
version mismatch, daemon restart, slow consumers and ambiguous input acknowledgment.

## M5 — macOS MVP release
Dependencies: M1–M4. A local-only preview can ship after M3 with its narrower scope stated.
- Add scoped CI for Swift modules, protocol compatibility, Rust daemon, and native builds.
- Record baseline launch/input latency and resource measurements; set budgets from evidence.
- Complete first-party MIT and dependency notice review, accessibility and recovery checks.
- Prepare signing, notarization, packaging, update strategy and user documentation.
Exit: a clean macOS machine installs and runs the signed artifact; local workflows work
offline without an account; bundled notices and source license match the shipped artifact.
Publishing releases and provisioning paid resources remain explicit delivery decisions.

## M6 — Optional cloud services
Dependencies: M4 and a demonstrated need for discovery or relay.
- Add identity/device enrollment, revocation, and expiring presence leases.
- Add optional web device management and short-lived scoped relay credentials.
- Implement authenticated encrypted transport using established protocols.
- Apply quotas, payload limits, rate limits, retention policies and operational metrics.
Exit: cross-account/device access and replayed/expired grants are rejected; revocation
closes access within a documented bound; cloud failure does not break local terminals.
Verify relay confidentiality against the actual threat model before production use.

## M7 — iOS client
Dependencies: M4; M6 only for cloud discovery/relay features.
- Validate a permissive mobile terminal renderer and reuse remote client contracts.
- Add remote session selection, input/resize, reconnect and secure credential storage.
- Handle mobile backgrounding and explicit writer handoff.
Exit: real-device checks cover network switching, suspension/resume, Unicode input,
keyboard/accessibility behavior, revoked devices and daemon/client version mismatches.

## Validation discipline
Test public behavior and failure boundaries rather than mirroring implementation details.
Use core model tests, original protocol fixtures, PTY integration tests and focused native
smoke checks. Add regression coverage for reproduced defects. Linux/portable checks do
not prove macOS UI, signing, or iOS behavior; report each evidence boundary separately.
