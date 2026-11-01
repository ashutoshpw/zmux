# Status

**Updated:** 2026-11-01

## Implemented slice: Herdr near-black appearance

The shared native palette in `apps/macos/Sources/WorkspaceViews.swift` has been changed to the requested near-black treatment. `WorkspaceRowView` now uses a neutral selected row and label-based text/icon colors, while the blue accent remains on the drag insertion marker.

| Token | Source value | Approximate appearance |
| --- | --- | --- |
| `window` | calibrated white `0.055` | `#0E0E0E` |
| `sidebar` | calibrated white `0.075` | `#131313` |
| `terminal` | RGB `24, 24, 26` | `#18181A` |
| `chrome` | calibrated white `0.085` | `#161616` |
| `separator` | white at alpha `0.085` | subtle translucent divider |
| `selection` | calibrated white `0.14` | `#242424` |
| `selectionBorder` | white at alpha `0.05` | subtle translucent edge |
| `dropInsertion` | RGB `0.24, 0.55, 1.0`, alpha `0.45` | `#3D8CFF` at 45% |
| `subdued` | calibrated white `0.64` | `#A3A3A3` |
| `success` | RGB `0.28, 0.78, 0.46` | `#47C775` |
| `inactive` | calibrated white `0.38` | `#616161` |

The approximate hex values are rounded display equivalents; the source `NSColor` values above are authoritative. Keep selection visually neutral, preserve blue for drop insertion, and retain status colors as semantic indicators. Palette appearance must not be used as evidence of provider activity, connection state, or SSH health.

## Review and verification

- Independent review confirmed that explicit ANSI black is retained by design; this palette update changes native surfaces and selection styling, not explicit terminal ANSI colors.
- Independent local lifecycle review found no blockers; the P3 safe visibility limitation below remains documented.
- Dark-slice hosted smoke checks passed. The full `scripts/check` retry passed on Linux after the lifecycle fixture change; log: `/tmp/zmux-lifecycle-check-r3.log`.
- Packaged-app screenshots for the dark and lifecycle slices were inspected. The screenshot compositor was unavailable, accessibility save was skipped, and no physical Mac verification was performed.

## Dark slice artifact

- Source commit: `b38e7f09341b15567550b7b3fd5a4ac1d92f4813`.
- Workflow run: [41283271682](https://github.com/ashutoshpw/zmux/actions/runs/41283271682).
- Download: [workflow artifact 13075045356](https://github.com/ashutoshpw/zmux/actions/runs/41283271682/artifacts/13075045356).
- Inner ZIP SHA-256: `191f5f43c326f96e736de4441f07465e3920c3bdfe6ac2e6d810a0bb73cf539c`.
- Target: universal macOS 14, arm64 and x86_64.
- Signing: ad hoc. Notarization: none.

## Implemented slice: local lifecycle evidence

Rust now owns a bounded registry for typed local Claude hook evidence, bound to the exact desktop namespace, terminal surface/session, provider session, adapter runtime, and generation. Hook capabilities stay out of snapshots. Same-UID desktop control can submit these claims; this is not provider-process authentication, remote-provider authentication, or provider replay evidence.

The opt-in Claude adapter reports prompt submission, ambiguous attention candidates, confirmed permission/elicitation waits, tool-driven resumption, stopped turns, and session loss. A Stop hook does not establish Idle because parallel Stop hooks may still continue the turn; only a later `idle_prompt` notification is used for Idle. The worker polls freshness even without other snapshot changes. Stale evidence becomes Unknown, later accepted current-generation evidence restores Connected, and disconnected or terminal capabilities cannot be revived. Exact terminal/session replacement and stale event fencing remain in Rust.

`SessionEnd` reasons `clear` and `resume` become Unknown/evidence-lost rather than Disconnected. The existing adapter shutdown and inbox-retirement behavior remains in place, so lifecycle tracking does not resume after those in-session transitions on the same wrapper run.

## Lifecycle verification

- `cargo fmt --all` passed.
- `cargo test -p zmux-desktop-runtime agent_lifecycle` passed the typed hook normalization and controlled-clock freshness/recovery/disconnect test.
- `cargo test -p zmux-desktop-runtime --test agent_lifecycle` passed the control-socket test for exact owner checks, generation/sequence fencing, terminal registration rejection, and evidence-loss replacement.
- `cargo test -p zmux --test provider_hooks` passed all 6 tests, including plugin lifecycle and nonblocking hook ingress.
- These are local Linux source checks. The full `scripts/check` retry passed after the latest fixture change; output is recorded at `/tmp/zmux-lifecycle-check-r3.log`. The dark-slice artifact above does not verify the lifecycle changes; see the separate lifecycle artifact below. Real Claude provider execution remains unverified. The native sidebar, SSH lifecycle, alerts/sounds, and physical-Mac evidence are also pending.

Independent lifecycle review recorded a **P3 safe visibility limitation**: when Claude self-starts a turn without a `prompt_id`, the lifecycle remains Unknown. Later tool events do not carry enough identity to establish which run they belong to, so they cannot safely create or promote that run. This can under-report activity, but avoids asserting a lifecycle state without run evidence.

## Failed lifecycle hosted attempt (superseded)

- Workflow [41285971536](https://github.com/ashutoshpw/zmux/actions/runs/41285971536) failed on macOS because a new control-socket test placed its Unix-domain socket under the long macOS `TMPDIR`, triggering the socket path / `SUN_LEN` limitation.
- The test fixture was moved to a short path under `/tmp`; the focused test passed after that change.
- The full local `scripts/check` retry passed; output is recorded at `/tmp/zmux-lifecycle-check-r3.log`. This failed hosted attempt was superseded by the successful lifecycle workflow recorded below.

## Lifecycle artifact

- Source commit: `37eaf32e578cae1e6893dfb0715cfb7fe9f89674`.
- Workflow run: [41286844041](https://github.com/ashutoshpw/zmux/actions/runs/41286844041).
- Download: [workflow artifact 13075709288](https://github.com/ashutoshpw/zmux/actions/runs/41286844041/artifacts/13075709288).
- Inner ZIP SHA-256: `bcda46cc70e2698b9aaafd2114288e666822915b6176cebca80c209cf2ec3feb`.
- Target: universal macOS, minimum macOS 14, arm64 and x86_64.
- Signing: ad hoc. Notarization: none.
- Local and hosted checks and packaged smoke passed; packaged screenshot was inspected. Compositor capture was unavailable and accessibility save was skipped.
- Real Claude provider execution and physical Mac verification remain unverified.

## Next implementation hazards — open

These are constraints for the remaining work; they are not implemented or verified by the slices above.

- Serialize an explicit empty lifecycle collection in snapshots. Omitting the field when there are no records can leave consumers unable to distinguish an empty current state from a missing update.
- Filter lifecycle records and updates by the exact window and namespace so an agent cannot appear in another window or namespace.
- Deliver agent lifecycle updates independently of workspace `modelRevision`; an agent state change must not wait for an unrelated workspace model change.
- The current selection command accepts a numeric surface identifier. Add an owner-side action fenced by the full window/namespace/workspace/pane/surface/session identity, and revalidate it at execution so stale identities fail without selecting a replacement surface.
- Keep the Agents list independently scrollable and preserve existing workspace drag-and-drop behavior and hit testing when the sidebar sections coexist.
- Keep the empty-agent state visually neutral; an empty list must not imply an alert, connection failure, or provider activity.

## Pending

- The dark appearance slice has an artifact and hosted verification record, but compositor capture, accessibility save, and physical-Mac proof are unavailable as noted above.
- Verify the lifecycle against a real Claude provider and on a physical Mac; implement and verify the native sidebar, truthful SSH status, exact existing-surface selection, and Alerts preferences, notifications, sounds, and deduplication described in `plan.md`.

The larger live-agent feature remains in progress; agent lifecycle, SSH integration, and alerts/sound behavior remain open.
