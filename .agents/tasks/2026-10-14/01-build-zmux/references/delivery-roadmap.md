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
- Establish a Cargo workspace for shared Rust logic and a thin Swift/AppKit host.
- Prototype Rust/Swift command/event and screen-data exchange; verify ownership, callback
  threading, errors, cancellation, view disposal, and supported target architectures.
- Measure idle and one-session CPU, resident memory, input latency, UI event-loop delay,
  repaint frequency, and PTY cleanup on the macOS prototype. Set documented numerical
  budgets and supported session/scrollback limits before expanding to multiple sessions.
- Establish a macOS build machine and minimal build instructions; check name availability.
Exit: native prototype displays an interactive shell with keyboard input, resize, Unicode,
and clean shutdown; engine/FFI decisions and license inventory have concrete evidence.
Engine ownership permits the proposed worker/render split; cross-language lifecycle checks
pass, and benchmark workload, machine, builds, measurements, and budget rationale are recorded.
If embedding is unsuitable, evaluate another permissive engine before expanding the app.

## M1 — One working terminal
Dependencies: M0.
- Create app shell, engine adapter, PTY ownership, focus and clipboard behavior.
- Keep parsing/I/O on session workers and coalesce screen updates into native rendering.
- Establish bounded queues/scrollback and cancellation without main-thread blocking waits.
- Support shell selection, working directory, exit status, and explicit error presentation.
- Respect platform permissions and avoid logging terminal content.
Exit: interactive shell, resize, copy/paste, IME and Unicode work on a real macOS build;
repeated open/close leaves no app-owned orphan PTYs/processes or obvious resource growth.
Sustained output cannot stall window interaction; the one-session workload meets M0 budgets.

## M2 — Workspace model and persistence
Dependencies: M1.
- Implement split tree, tabs, workspaces, focus traversal, rename and close policies.
- Implement workspace/storage rules in Rust; Swift observes state and submits shared actions.
- Give sessions fair processing/render scheduling, with bounded visible-screen snapshots.
- Route UI and keyboard commands through shared actions.
- Add versioned snapshot persistence and safe restoration.
Exit: model checks cover invalid actions, focus after closure, split-tree invariants and
out-of-order effect completion; native checks cover layout, accessibility, keyboard-only
navigation and restart. Corrupt/older snapshots recover without silently losing the original.
Multiple busy panes stay within agreed budgets while an idle pane remains interactive;
closing panes during queued events does not apply stale updates or leave active workers.

## M3 — Local automation and CLI
Dependencies: M2; protocol design can begin after M0.
- Define versioned schema and original contract fixtures.
- Implement local socket authentication, bounded parsing, dispatch, and subscriptions.
- Add CLI list/create/focus/split/close and explicit terminal-input operations.
- Add scoped agent notifications without giving notification payloads execution authority.
Exit: UI and CLI produce equivalent state for equivalent actions; integration checks cover
unauthorized peers, oversized/invalid input, stale IDs, concurrent requests and disconnects.
Slow subscribers and saturated output cannot starve workspace commands or grow event queues
without bound. Enforce command deadlines and report explicit overload/cancellation errors.
Document automation's same-user trust boundary and destructive command semantics.

## M4 — SSH remote sessions
Dependencies: M3; daemon prototyping may follow protocol stabilization.
- Implement remote PTY daemon, framed transport, session IDs, bounded replay and cleanup.
- Use SSH host verification and explicit agent installation/version negotiation.
- Implement reconnect, resize, exit reporting and predictable retention limits.
- Model connection generations, bounded retry/backoff, cancellation and state transitions.
- Exercise multiple sessions through network loss, high-volume output and sleep/wake;
  verify local UI and unrelated sessions remain usable through failure and recovery.
Exit: a two-host check proves commands execute on the remote host; connection loss preserves
the remote process, reconnect restores available output, and replay gaps are reported.
Unauthorized session access fails; failed SSH never executes commands locally. Validate
version mismatch, daemon restart, slow consumers and ambiguous input acknowledgment.
The stress matrix in [session-reliability.md](session-reliability.md) passes its responsiveness,
memory, queue, worker and retry budgets. Record soak duration/session count before running;
inspect cleanup and resource trends afterward, not only whether the connection recovered.

## M5 — macOS MVP release
Dependencies: M1–M4. A local-only preview can ship after M3 with its narrower scope stated.
- Add scoped CI for Rust core/CLI/agent, Swift bridge/UI integration, protocols, and native builds.
- Re-run M0–M4 resource workloads with the release build and investigate budget regressions.
- Verify UI/process diagnostics identify overload/reconnect failures without retaining terminal
  content, commands, credentials, or other secret material by default.
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
- Prototype linking the Rust remote client into the native iOS app, with bridge cancellation
  and backgrounding behavior validated before assuming desktop binary/runtime reuse.
- Add remote session selection, input/resize, reconnect and secure credential storage.
- Handle mobile backgrounding and explicit writer handoff.
Exit: real-device checks cover network switching, suspension/resume, Unicode input,
keyboard/accessibility behavior, revoked devices and daemon/client version mismatches.

## Validation discipline
Test public behavior and failure boundaries rather than mirroring implementation details.
Use Rust core model tests, original protocol fixtures, PTY/FFI integration tests and focused native
smoke checks. Add regression coverage for reproduced defects. Linux/portable checks do
not prove macOS UI, signing, or iOS behavior; report each evidence boundary separately.
Resource use and stability claims need measured native workloads. The user's cmux hangs
are motivation for these gates, not evidence that Swift caused them or Rust eliminates them.
