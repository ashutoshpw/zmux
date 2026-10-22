# Persistent scheduled remote tmux messages

**Date:** 2026-10-22
**Task:** 02-scheduled-tmux-messages

## Objective
Let a user schedule literal text for a specific remote tmux pane and manage the
pending queue and delivery history from Zmux CLI and the macOS app. Once the
remote service acknowledges a job, it must remain scheduled while the Mac, app,
and SSH client are disconnected, provided the remote service stays available.

## Context
Immediate `send-text` targets a Zmux terminal surface and writes up to 4096 UTF-8
bytes without adding Enter; it does not identify a remote tmux pane. Managed SSH
operations can reuse the live private master without authenticating again, but a
manually typed `ssh` command has no managed connection metadata. Existing tmux
identity records cover local tmux only, and Zmux has no persistent scheduler.
The tmux-web scheduler is a behavior reference only; its mutable name/index target
and implementation are not to be copied.

## Approach
- Use a per-remote-user Rust helper with private durable storage and an SSH-run,
  same-user control protocol. No public listener or cloud dependency.
- Start with Linux and a systemd user service as a proposed platform target.
  Setup is an explicit action; report service, compatibility, and linger needs.
- Pin each job to a verified remote account, tmux socket/server incarnation,
  session, and pane. Follow the same pane across window, index, name, layout, and
  focus changes while it remains in the bound session on that server; fail if it
  leaves that session or the server identity changes. Window identity is display data.
- Expose one shared queue/history to CLI and AppKit. Editing message and/or time
  is atomic on pending jobs; changing the target requires creating a new job.
- Offline AppKit edits/cancels remain local drafts; CLI mutations fail without
  remote acknowledgement. On timeout reconcile by request ID and task version.
- Claim jobs durably before sending and enforce a five-second dispatch grace.
  Expired jobs are missed without input; if text is accepted but optional Return
  becomes stale, record a partial unknown outcome and never replay either phase.
- Add one `schedule` entry with all verbs in the existing command registry so
  `zmux --help` discovers it; route nested help offline before any SSH/app request.

## Steps
- [x] Specify remote identity, service lifecycle, persistence, time, and dispatch contracts.
- [x] Implement the remote helper, durable queue, versioned edits/cancellation, and timer.
- [x] Integrate managed SSH control and the CLI queue/history commands and help catalog.
- [x] Add native schedule list/editor/history views backed by the remote helper protocol.
- [x] Document the CLI and macOS entry points, service lifecycle, limits, and receipts.
- [x] Complete local Rust, CLI, isolated Linux/tmux/SSH, and packaging validation.
- [ ] Build the packaged Mac app, run its schedule smoke, deliver the download, and complete native acceptance.

Frozen-tree local validation passed. The native smoke fixture is present in source
but has not been run; packaged Mac artifact, download, and full acceptance remain
pending.

## Acceptance criteria
- [ ] Scheduling requires a running supported remote service and a verified live target;
  there is no local timer fallback. The job survives loss of the Mac/app/SSH client.
- [ ] Jobs follow only the immutable pane inside its bound session/server; leaving that
  scope fails, and no selected, named, or index-matched pane is substituted.
- [ ] Pending edits atomically replace message/time; stale versions and edit/fire races fail clearly.
- [ ] Disconnected AppKit edits/cancels remain drafts and CLI mutations fail without
  remote acknowledgement; timeouts reconcile by request ID/version before retry.
- [ ] Restart policy, duplicate requests, uncertain outcomes, and partial writes are explicit;
  uncertain dispatch is never replayed automatically.
- [ ] Dispatch respects the five-second grace: late pre-input tasks are `missed` with no
  input, while a stale optional Return after accepted text is partial `outcome_unknown`
  with no Return sent or automatic replay.
- [ ] CLI and AppKit show and edit the same pending jobs and history.
- [ ] `zmux --help`, `zmux help schedule`, `zmux help schedule edit`, `zmux schedule --help`,
  nested subcommand help, and the machine catalog describe only implemented behavior.
- [ ] Meaningful future checks exercise real isolated tmux/SSH/service owners, not mocked
  tmux delivery semantics; platform and physical-host limits are reported separately.

## References
- [references/remote-contract.md](references/remote-contract.md) — identity, lifecycle,
  persistence, time, job states, and delivery safety.
- [references/cli-native-delivery.md](references/cli-native-delivery.md) — CLI/help shape,
  shared native ownership, implementation phases, and future validation contract.
