# Native Live Agents Sidebar

**Date:** 2026-11-01  
**Task:** 01-live-agent-sidebar

## Objective
Adapt the attached sidebar design so local and SSH workspaces remain at the top and provider-backed live agents appear below, grouped by truthful activity/connection state. Selecting an agent focuses its exact existing terminal surface and session; new completion and needs-attention events can notify and sound once on the local Mac.

The requested appearance also follows Herdr's near-black treatment across the native window, sidebar, terminal, and chrome, with a restrained charcoal selected row instead of a blue selection fill.

## Context
- Reference: `/home/ashutosh/.t3/userdata/attachments/9657c486-910d-4d55-8e0c-91199666d982-60e4d10d-ed6a-43c4-b0e3-d667864512d0.png` — dark narrow rail with “Spaces” above compact “Agents” status groups and provider/workspace details.
- Alerts reference: `/home/ashutosh/.t3/userdata/attachments/9657c486-910d-4d55-8e0c-91199666d982-bc99ab53-d909-4fe4-aa1c-028e687da8a1.png` — Herdr’s Alerts pane, with a “Play sounds” switch, “An agent needs you” and “An agent finished” sound-preview rows, and custom `sounds.blocked` / `sounds.done` MP3 guidance.
- Native sidebar rows are rebuilt from the active window’s workspace order. Terminal tabs belong to panes and already select a surface through the Rust workspace model.
- Rust `agent_messages` is a durable message inbox, not live provider presence; its current capability contract says `provider_hooks`, `wake`, and `native_inbox` are false, and its consumer identity is not an authenticated provider.
- Existing Claude shell hooks can surface generic completion, failure, and prompt notices through local or SSH PTYs. The new opt-in local adapter binds hooks to the exact local terminal/session and adds adapter-owned runtime and generation fences, but its sequence is not a provider replay cursor and the same-UID control path does not authenticate provider process identity. SSH lifecycle remains unsupported; current native notification delivery also has no app-level sound preference.

## Approach
Rust owns provider lifecycle/status and an opt-in provider-specific adapter; Swift/AppKit presents the grouped list and a native Alerts settings surface. Bind every row to namespaced window/workspace/pane/surface/session identity plus provider runtime and run generations, never titles, paths, or row positions. Separate work state from connection state: show provider-reported Working/Waiting/Idle states, and distinguish confirmed Disconnected from Unknown when evidence is absent or stale. Do not infer liveness from terminal output, filesystem changes, or elapsed time alone. SSH status requires an explicit authenticated remote adapter and must respect the existing SSH/session boundary. Persist a master “Play sounds” preference and separate needs-attention/completion sound choices with preview; audio runs on the local Mac for both local and authenticated remote agents while visual notifications remain available when audio is muted. Reuse both references’ spacing, subdued labels, compact icons, activity marks, and selected-row treatment with native assets. The shared native palette uses near-black Herdr surfaces and a neutral charcoal selection; reserve blue for drag insertion and retain distinct semantic status colors.

## Steps
- [x] Apply the requested Herdr near-black treatment to the shared native palette and neutralize selected-row styling; independent review and Linux `scripts/check` are recorded in `status.md`, with packaged native verification still pending.
- [ ] Define the provider event, identity, generation/replay, freshness, disconnect, and capability contract; map only new `needs_user`/blocked/failure and completed events to alert sounds; keep unsupported providers explicitly Unknown.
- [x] Add bounded Rust-owned live-agent registration and provider-specific opt-in lifecycle ingestion; fence stale events and remove or mark records when their exact terminal/session closes. This slice covers local Claude only, with same-UID evidence and no provider replay or process-authentication guarantee; see `status.md`.
- [ ] Add a native Agents section below existing workspace rows with grouped counts/disclosure, provider and workspace context, loading/empty/unsupported/disconnected states, independent scrolling, keyboard navigation, and accessibility labels.
- [ ] Add Alerts preferences with a persisted master sound switch, separate needs-attention/completion sound choices and preview controls; setting audio off leaves the visual notification and inbox item intact.
- [ ] On selection, revalidate the exact identity and use existing workspace/surface selection so the same terminal is shown; stale or closed targets report unavailable without spawning a replacement.
- [ ] Verify lifecycle transitions, event dedupe/replay/reconnect, local and SSH routing, both sound previews and mute behavior, visual notification delivery, native layout and selection; then run `scripts/check` and the packaged universal macOS acceptance workflow and record artifact hash, source SHA, architectures, minimum OS, and signing/notarization evidence.

## Acceptance criteria
- [ ] Each visible activity state is backed by current, provider-specific lifecycle evidence; Unknown and Disconnected never appear as Idle.
- [ ] A new provider event sequence plays the needs-attention sound once when an agent newly needs the user and the completion sound once when a run finishes; Working, Waiting, Idle, Unknown, Disconnected, repeated events, snapshot refreshes, and replayed events do not sound.
- [ ] A provider/run generation change admits a fresh lifecycle; reconnect recovery fences older generations, deduplicates event IDs, reconciles state without replaying historical sounds, and does not turn transport loss into Idle or a new alert.
- [ ] The Alerts master switch and each selected sound persist across app relaunch; both preview buttons play only their selected sound. Muting suppresses agent-event audio in foreground/background while the visual in-app and authorized system notifications still appear; a single event never produces duplicate audio.
- [ ] Local and authenticated remote provider events follow the same settings and dedupe rules, with audio played on the local Mac and remote state unavailable/Unknown after evidence is lost. Terminal OSC notices alone cannot make an agent live or cause a second provider lifecycle alert.
- [ ] Selecting an agent focuses the matching existing window/workspace/pane/surface/session, including when another workspace or split pane is currently focused; no new terminal is created.
- [ ] Closed, replaced, stale-generation, and cross-window targets cannot select another agent’s terminal. SSH state follows an explicit authenticated adapter and visibly reports loss of connection.
- [ ] The sidebar remains usable at narrow window sizes and with many spaces/agents; grouped rows, keyboard selection, VoiceOver labels, and loading/empty/error states are covered by native acceptance evidence.
- [ ] `scripts/check` and the required downloadable universal macOS build pass; source metadata, checksum, target architectures, minimum OS, signing/notarization status, and packaged UI evidence are recorded.

## References
- `apps/macos/Sources/App.swift:896-940, 1563, 1807-1823` — workspace rows and existing workspace/surface selection actions.
- `apps/macos/Sources/WorkspaceViews.swift:520-565, 840-882, 1000-1008` — sidebar list and pane terminal-tab selection.
- `crates/workspace-core/src/navigation.rs:180-188` — selecting a surface also focuses its pane/workspace.
- `crates/desktop-runtime/src/agent_messages/domain.rs:149-177` and `crates/desktop-runtime/src/control.rs:932-938` — current terminal discovery and inbox capability limits.
- [references/alerts-sounds.md](references/alerts-sounds.md) — screenshot details, existing local/SSH notice and notification paths, lifecycle/audio acceptance, and proof limits.
