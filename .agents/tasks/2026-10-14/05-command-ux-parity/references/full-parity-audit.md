# Whole-product source parity audit

**Date:** 2026-10-15. **Reference:** cmux `3ba653b9e5afb11d0bc578031142950d6136697e`.
**Target:** zmux `69342a8f036037736c6eb9a500e479ac41e32d93` on `main`.

## Conclusion

zmux is a functional early terminal/workspace application with selected native browser
and notification behavior. It is not a full cmux replacement. The short CLI help is a
symptom of missing command contracts and product owners, not simply missing prose.
Managed SSH, remote file/image transfer, durable sessions, agent workflows, browser
automation and cloud VM operations require substantive implementations.

The broader source audit is recorded across the reports below. It covers product
families, source entrypoints, owning components, current zmux differences, dependency
boundaries and acceptance scenarios. This is not a certification that every option
combination or every line of cmux source has been inspected or executed. Detailed
per-leaf executable contracts and numerical macOS budgets remain implementation gates
in P0. No feature is accepted as full parity merely because its name appears in a
ledger, help, shortcut enum or capability list.

## Evidence map

| Audit | Scope |
| --- | --- |
| [CLI/shared contracts](audit-cli-contract.md) | Argument grammar, offline help, UUID/refs/indices, caller defaults, socket selection/auth, output, errors, focus, startup, bounded control |
| [Terminal and topology](audit-terminal-ux.md) / [rows](audit-terminal-ux.json) | Emulation, input/IME, scrolling/selection/paste, links/images/find, tabs/panes/workspaces/windows, restoration, menus, gestures, accessibility |
| [Browser and content](audit-browser-content.md) / [rows](audit-browser-content.json) | Browser UI and automation, profiles/downloads/permissions, content viewers, files/search, settings, shortcuts, custom sidebars/palette |
| [Agents and remote](audit-agents-remote.md) / [rows](audit-agents-remote.json) | SSH/mosh/multiplexers, transfer/reconnect/relay, agent hooks/messages/launchers, notifications, telemetry, events/automation |
| [Cloud and platform](audit-cloud-platform.md) / [rows](audit-cloud-platform.json) | Auth/team, VM/cloud nested operations, mobile/iOS/simulator, diagnostics/security/distribution and retained PR #1 requirements |
| [Cross-cutting additions](audit-cross-cutting.md) / [rows](audit-cross-cutting.json) | Computer Use, enterprise policy, project pane and quit lifecycle found during independent review |
| [Source identifier catalog](audit-source-catalog.json) | Main and early CLI dispatch candidates; advertised RPC identifiers including simulator/debug additions |
| [Inventory reconciliation](audit-reconciliation.json) | Routing of all original 428 help rows and 171 shortcut identifiers to current review owners; current CLI support boundary |
| [SSH/transfer contract](ssh-transfer-contract.md) | Detailed connected journey from managed SSH through drop/paste/upload to origin-bound path insertion |

The five area ledgers contain 172 overlapping feature-family records, alongside the
shared CLI contract table. None is accepted as full parity. The catalog contains 217
classified top-level dispatch candidates and 571 RPC identifiers; those are different
dimensions, not additional implemented features.

Feature families deliberately overlap at shared boundaries: native paste and CLI
paste, browser topology and terminal topology, cloud-hosted sessions and ordinary SSH.
Do not sum family rows into a percentage of product completion. `verified-subset` in
area ledgers means inspected source has focused tests/previous evidence for a bounded
subset; it is not complete compatibility or a fresh macOS runtime result.

## Most consequential findings

| Area | Present in zmux | Remaining parity gap |
| --- | --- | --- |
| CLI | Offline help/version, small same-user JSON socket client | Shared registry, most command families, compatible options/aliases/defaults/output, stable targets and caller routing |
| Terminal | PTY workers, basic vt100 parsing/rendering, bounded history, wheel/mouse routing, title/cwd metadata | Engine/protocol breadth, search, selection/copy completeness, graphics, links, richer keyboard/config behavior and native accessibility validation |
| Tabs and topology | Independent tabs, split tree, move/reorder, close/reopen, fresh-shell retry | Multiwindow, groups, focus history/directional navigation, richer placement/metadata/actions, canvas/layouts and full recovery semantics |
| Notifications | BEL/OSC/explicit in-app inbox, unread ownership, focus acknowledgement, Dock badge | OS delivery/reply/actions, agent-hook attention, richer target/clear scopes, status/progress/log channels and unread/prompt navigation |
| Browser | Embedded WKWebView, navigation/find/zoom and basic open/navigate CLI | Automation, profiles/import/state, downloads, page permissions/dialog handling, advanced developer modes, remote/mobile browser control |
| Persistence/settings | Limited saved native preferences; in-process close history | Session/window/layout persistence, export/import/checkpoints, config/schema/reload, themes, configurable shortcuts, customization |
| Content/automation | No comparable integrated owners | Markdown/diff/editors/file explorer/search, reviews/vault, events/replay/rules/todos, capture and tmux compatibility |
| Agents | Notifications can be produced explicitly | Durable messaging/inbox/replies, provider hooks, launch/resume/recovery, hibernation, team wrappers and account integration |
| Remote | A local shell can launch installed SSH manually | Managed SSH identity, host/route handling, daemon/reconnect/relay, origin-bound drop/paste transfer, mosh/tmux/session movement |
| Cloud | Identity/device service and native auth foundations | Live deployment, CLI auth, complete VM/network/file/snapshot/agent/browser/mobile services, organizational and security requirements |
| Distribution | Prior macOS universal development artifact and smoke checks | Fresh acceptance for each added journey, Intel runtime, accessibility/physical input, signing/release/update coverage and resource/remote-stress budgets |

## Why the earlier approach missed parity

The initial ledgers enumerated help commands and shortcut identifiers. They did not
establish each dispatcher's target resolution, state transitions, platform effects or
failure handling. Several similarly named zmux operations have different grammar or
scope. Browser UI operations also do not imply corresponding CLI automation exists.

This audit traces the owning families and includes features outside the supplied help:
window/canvas/layout/workspace-group namespaces, project/content routes, mobile, VPN,
billing, Computer Use, managed-device policy, native menus/context menus, file/image
paste/drop and remote relay policy. Every discovered top-level candidate has a classified
review owner; this does not turn internal helpers or RPC discovery into execution proof.
Internal lifecycle helpers and legacy aliases are retained as discovery evidence;
they are not automatically public commands to advertise. Provider passthrough grammars,
settings keys, selectors and numeric indices are dynamic domains, not finite lists of
new behavior. Capability discovery does not grant remote authorization.

## Implementation order and acceptance ownership

1. **P0 — shared contracts.** Original Rust registry, typed identity/caller resolution,
   focus policies, command/result/errors, configuration and source-ledger drift checks.
   Preserve existing numeric command compatibility intentionally. Measure a named Mac
   under idle, active output, hidden tabs and stalled remote conditions before setting
   budgets. Registry work must not create successful no-op commands.
2. **Priority remote journey — independent of cloud.** Managed SSH and remote file/image
   drop/paste together. Connection descriptor, host-key/auth routing, bounded transfer
   worker, cancellation/progress and origin generation binding. Prove Unicode/spaces,
   jump hosts, auth failure, disconnect, tab-switch/close-during-upload and no local
   fallback on a real second host.
3. **P1/P2 — everyday terminal UX.** Close remaining input/selection/find/topology,
   multiwindow/focus/restore and notification lifecycle gaps. Native UI, menu, shortcut
   and CLI must share owners and produce the same target/state effects.
4. **P3/P4 — browser and persistence/customization.** Finish page lifecycle and data
   isolation, then browser automation/content/settings. Public WebKit feasibility for
   advanced APIs stays an explicit unresolved parity gate, not a silent omission.
5. **P5/P6 — automation/content/agents.** Bounded event store/replay, rules and metadata;
   content/project tools, durable agent messages, hooks/launchers/vault and provider
   adapters. Add Computer Use through its own signed helper and permission/approval boundary.
   Managed policy must apply across these owners and revoke live access when required.
6. **P7/P8 — cloud and platform.** Complete independent service and VM/provider adapters,
   mobile/iOS/simulator and advanced diagnostics/security. Preserve PR #1 product scope.
   Deploy only through GitHub Actions to `https://zmux.w3dev.app`; hosting/database
   selection remains deferred. Cloud is not deployed and that boundary must stay visible.

Every cohesive phase ends with scoped verification, a commit on main, updated per-feature
evidence and a GitHub Actions macOS artifact when executable behavior changes. Failures,
invalid input, stale targets, permissions, reconnect and cancellation are part of the
feature; a success-only demonstration is insufficient. Dependency-blocked features
remain incomplete. The original MIT implementation constraint applies to every phase.

## Audit verification and limits

The parent reconciles all initial inventory IDs, validates report schemas and source
path/line bounds, and checks cross-report contradictions. `scripts/check` passed on
2026-10-15: formatting, 45 Rust tests, clippy, dependency notices and the C ABI harness.
The license inventory continues to flag two notices for upstream-text review; its
check passed but that is not a legal clearance of future dependencies.

This change is documentation/metadata only. No app behavior was changed, no managed
SSH session or transfer was exercised, no cloud endpoint was deployed, and no fresh
native build was generated. The previous [native delivery receipt](native-delivery.md)
remains the artifact evidence for the existing subsets; it is not evidence for the
newly cataloged missing features. Physical trackpad/IME/VoiceOver, Intel runtime,
real browser permission/TLS flows, remote stress and numerical resource budgets
remain explicit acceptance gates.
