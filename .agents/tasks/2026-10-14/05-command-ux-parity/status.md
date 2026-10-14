# Status

- **State:** in-progress
- **Updated:** 2026-10-14

## Delivered slices

- P1: independent terminal tabs; ⌘T/⌘W/⇧⌘T and Recently Closed; move/reorder, grouped closes,
  fresh-shell restoration and retryable launch failures. Hidden sessions remain alive.
- P2: collapsible sidebar with saved visibility, Workspaces/Notifications tabs, attention badges,
  bounded bell/OSC/explicit notification state and generation-safe read/open/dismiss/clear.
- P3a: embedded public WebKit surfaces without PTYs, address/navigation/find/zoom controls,
  retained hidden-tab state, CLI open/navigate, frame-owned split layout and ratio retention.
- Original MIT implementation; no imported or translated cmux GPL/BUSL code/assets.

## Verification and download

Accepted runtime source: `7e9355b4dd820580443777ae6b3dd9a5fb0f4024`.
[Download macOS build](https://github.com/ashutoshpw/zmux/actions/runs/38647522249/artifacts/11932279010).
Local checks and hosted universal/native smoke pass; archive and both captured images inspected.
See [delivery receipt](references/native-delivery.md) for checksum, evidence boundaries and repairs.

## Open scope

- P0 exhaustive menu/gesture/dispatcher/options audit, shared command registry, UUID/typed refs,
  caller defaults/auth contracts and measured Mac resource budgets remain open. Initial ledgers
  contain 428 command entries and 171 source-discovered UI actions; they are not completion counts.
- Whole P1/P2/P3 phases remain incomplete: multiwindow/focus history/groups, original pane restore
  anchors, richer attention/hook/status operations, browser automation/profiles/downloads and
  remaining UX/command contracts are tracked in their references and phase plan.
- P4–P8 persistence/customization, automation/content tools, agents, remote/cloud and platform
  behavior remain open. Reopening creates fresh local shells, not resurrected jobs or remote sessions.
- Interactive drag/IME/VoiceOver, Intel runtime, actual HTTP/TLS/permission journeys and numerical
  resource/remote-stress evidence remain open. Cloud callback smoke is not live cloud sign-in.
- Cloud is not deployed. Origin is https://zmux.w3dev.app; provider/database selection is deferred.
  All deployments must use GitHub Actions.

## Next action

Continue P0 command registry/target contracts and remaining P1 navigation/window/recovery actions;
then close P2 attention and P3 browser gaps against the linked ledgers. Preserve native artifact
and visual gates, and commit cohesive validated slices. Full parity is still the objective.
