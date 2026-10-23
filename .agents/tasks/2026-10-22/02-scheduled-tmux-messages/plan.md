# Persistent scheduled remote tmux messages

**Date:** 2026-10-22
**Task:** 02-scheduled-tmux-messages

## Objective

Schedule literal text for a verified remote tmux pane and manage its pending
queue and delivery history from the Zmux CLI and macOS app. Once acknowledged
remotely, a job remains scheduled while the Mac, app, and SSH client are offline.

## Context and design

- Existing send-text targets a Zmux terminal surface, not an identified remote
  tmux pane; Zmux had no persistent scheduler.
- Use a per-remote-user helper with private durable storage and same-user SSH
  control. Linux/systemd user service is the initial proposed platform.
- Pin a job to the verified remote account, tmux server incarnation, session,
  and pane. Never substitute a selected, named, or index-matched pane.
- CLI and AppKit share the remote queue/history. Pending message/time edits are
  atomic; offline AppKit edits remain drafts; CLI mutations require remote ack.
- Claim durably before dispatch; enforce a five-second grace. Reconcile timeouts
  by request ID/version and never automatically replay uncertain input.
- One schedule command owns its verbs and offline nested help/catalog.

## Steps

- [x] Specify remote identity, lifecycle, persistence, time, and dispatch contracts.
- [x] Implement the remote helper, durable queue, edits, cancellation, and timer.
- [x] Integrate managed SSH control and CLI queue/history/help.
- [x] Add native schedule list, editor, and history views over the shared protocol.
- [x] Document CLI/AppKit entry points, service lifecycle, limits, and receipts.
- [x] Pass local Rust, CLI, isolated Linux/tmux/SSH, and packaging validation.
- [x] Pass final packaged scheduler visual acceptance and deliver the verified universal Mac artifact.

## Current extension

The user requested relative scheduling, countdowns in the list, and a Schedules tab
inside the main UI (including SSH workspace context menu), rather than a new window.

- [x] Rust-owned Schedules surface/action with exact SSH source binding and persistence.
- [x] Embedded native panel with relative input, presets and display-only countdown.
- [x] CLI relative list output, help and user documentation.
- [x] Focused behavioral checks, full portable gate, self-review and verified Mac artifact.
- [x] Strengthen source lifecycle/capacity and CLI relative/edit coverage; pass focused and full checks.
- [ ] Implement the selected native right inspector, verify its AppKit behavior and deliver a new universal Mac build.

A closed source leaves its panel offline; undo-close does not silently bind it to a
newly created SSH terminal. Only topology/source IDs are persisted, never message drafts.

## Current status

Relative scheduling and embedded Schedules tabs are complete. Source
`93f36fcab3ac89fa0fa32639a8b140f5bfd5066a` passed the complete Mac run
[39880328272](https://github.com/ashutoshpw/zmux/actions/runs/39880328272), including
10 focused AppKit resize cases and all 14 packaged scheduler checks. Personal
post-commit review and archive/screenshot inspection are complete.

Verified final artifact
[12471043165](https://github.com/ashutoshpw/zmux/actions/runs/39880328272/artifacts/12471043165)
contains `zmux-dev-universal-93f36fc.zip`, SHA-256
`045ad8b1378bbaf96edfa54728cc9572b2a74283a0347d21670bd735c6321765`.
The build is universal, requires macOS 14+, and is ad-hoc signed, not notarized.
See [status.md](status.md) for verification boundaries, receipts and prior history.

## Acceptance criteria

- [x] Scheduling requires a supported running remote service and a verified live target; no local timer fallback.
- [x] Jobs follow only the immutable pane within its bound session/server and fail outside that scope.
- [x] Pending message/time edits are atomic; stale versions and edit/fire races fail clearly.
- [x] Offline AppKit changes remain drafts; CLI mutations require remote ack; timeouts reconcile before retry.
- [x] Restart, duplicate, uncertain, partial-write, and five-second dispatch-grace outcomes are explicit.
- [x] CLI and AppKit show and edit the same queue and history.
- [x] Top-level, nested, and machine-catalog help describe implemented behavior offline.
- [x] Validation exercises real isolated service/tmux/SSH owners and reports native/host limits separately.
- [x] Packaged scheduler screenshots show legible message content, non-overlapping table columns, and usable heading spacing.

## References

- [scheduler-design-options.md](references/scheduler-design-options.md) — desktop proposals; option 1 is selected.
- [right-inspector-delivery.md](references/right-inspector-delivery.md) — native layout implementation, test contracts and artifact evidence.
- [remote-contract.md](references/remote-contract.md) — identity, lifecycle, persistence, time, and delivery safety.
- [cli-native-delivery.md](references/cli-native-delivery.md) — CLI/help shape, native ownership, and validation contract.

- [relative-tabs-review.md](references/relative-tabs-review.md) — post-commit review and validation boundary.
- [scheduler-test-coverage.md](references/scheduler-test-coverage.md) — test owners, authoring decisions and follow-up results.
