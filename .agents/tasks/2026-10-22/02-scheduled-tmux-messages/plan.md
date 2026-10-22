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
- [ ] Pass final packaged scheduler UI acceptance and deliver a verified universal Mac artifact.

## Current status

Source 4879579a7fbfe3a9acc416bc9998392776eb0f78 passed Swift checks for both
architectures, Linux helper builds, Rust/C ABI checks, universal app build,
terminal selection, and browser links. Run [39830501406](https://github.com/ashutoshpw/zmux/actions/runs/39830501406) failed only in the
packaged scheduler smoke after seven checks passed; cancellation-history timed
out. The packaged UI is exercised, but end-to-end acceptance is incomplete.

The initial-workspace repair is present in that source. Native fixes for edit-mode
reload ordering, requested-job priority, and the dirty-editor compare-and-swap base
are now frozen locally, with independent review clear. The macOS acceptance checker
also requires the dirty-refresh CAS evidence while retaining its existing five
Applied receipt gates. Root's local full check passed; a packaged run on the frozen
native changes and final artifact verification are still pending.

The 8f4e186 candidate archive passed structural checks only. The final artifact
and download are not yet verified. See status.md for the CI timeline, checks,
artifacts, and deferred risks.

## Acceptance criteria

- [ ] Scheduling requires a supported running remote service and a verified live target; no local timer fallback.
- [ ] Jobs follow only the immutable pane within its bound session/server and fail outside that scope.
- [ ] Pending message/time edits are atomic; stale versions and edit/fire races fail clearly.
- [ ] Offline AppKit changes remain drafts; CLI mutations require remote ack; timeouts reconcile before retry.
- [ ] Restart, duplicate, uncertain, partial-write, and five-second dispatch-grace outcomes are explicit.
- [ ] CLI and AppKit show and edit the same queue and history.
- [ ] Top-level, nested, and machine-catalog help describe implemented behavior offline.
- [ ] Validation exercises real isolated service/tmux/SSH owners and reports native/host limits separately.

## References

- [remote-contract.md](references/remote-contract.md) — identity, lifecycle, persistence, time, and delivery safety.
- [cli-native-delivery.md](references/cli-native-delivery.md) — CLI/help shape, native ownership, and validation contract.
