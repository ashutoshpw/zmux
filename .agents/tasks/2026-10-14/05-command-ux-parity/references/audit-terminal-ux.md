# Terminal, topology, and native UX parity audit

Source audit on 2026-10-15. Baselines: cmux
`3ba653b9e5afb11d0bc578031142950d6136697e`; zmux
`69342a8f036037736c6eb9a500e479ac41e32d93`. The machine-readable findings are in
[`audit-terminal-ux.json`](audit-terminal-ux.json).

## Scope and boundaries

This report covers terminal engine, rendering, input, selection, scrollback, search,
clipboard, inline images and terminal drop/paste behavior; window, workspace, pane,
tab, group, layout, canvas, focus, close/reopen and persistence behavior; and native
menus, context menus, shortcuts and accessibility for those areas.

Notification semantics and hook delivery belong to the agents/remote/attention audit.
Browser content, browser automation and settings behavior belong to their domain audits.
Where those areas meet terminal or topology state, this report records only the shared
boundary. It does not import or translate cmux code, tests, strings or assets.

The review traced representative owners, dispatchers, action handlers, failure paths,
existing tests and the accepted native delivery receipt. It reconciled the original
428-row command inventory and 171-row UI action inventory with the later lexical source
catalog. This is a broad source-based audit, not an exhaustive line-by-line review of
either repository.

## Result

The ledger contains 62 feature families. None is classified as fully verified.
`verified-subset` means that a narrower behavior has executable or native receipt
evidence while the row's complete parity contract remains open.

| Area | Total | Verified subset | Partial | Implemented, unverified | Missing |
| --- | ---: | ---: | ---: | ---: | ---: |
| Terminal | 25 | 8 | 7 | 2 | 8 |
| Topology and recovery | 27 | 11 | 3 | 1 | 12 |
| Native UX | 10 | 0 | 5 | 2 | 3 |
| **Total** | **62** | **19** | **15** | **5** | **23** |

Thirty-five rows are P0, 22 are P1 and five are P2. Status is intentionally
conservative: source existence, a parser branch, a menu row or an advertised RPC name
does not establish compatible behavior.

## Terminal findings

Zmux has useful tested foundations: independent PTYs, ordered read/write cleanup, basic
ANSI rendering, kernel resize, common key input, scrollback navigation, alternate-screen
separation, shell title/cwd updates and inspectable exited output. The accepted macOS
receipt also covers focused native input, resize, wheel history, Command-Home/End and
accessibility content updates for its pinned source.

That evidence does not approach the full terminal contract. The adapter describes itself
as a feasibility parser rather than a complete terminal engine. Protocol breadth,
advanced keyboard modes, mouse reporting, selection edge cases, text attributes,
overload recovery and cross-window input ownership still need explicit fixture matrices.
IME and bracketed paste have implementation paths but no current behavioral evidence in
this audit.

The clearest absent user journeys are keyboard copy mode, terminal search, clear-screen
with retained history, inline graphics, file/image/screenshot paste, terminal drop,
link/context actions and appearance customization. Pointer selection and clipboard copy
are partial. Manual IME and VoiceOver remain open in the native receipt, so source-level
conformance must not be reported as native usability.

Zmux's bounded `read-screen` and `send-text` commands are useful primitives, but they do
not cover cmux's tmux-compatible capture, scrollback clear, named buffers, pipe-to-shell,
bracketed buffer paste, respawn or display-message contracts. The detailed compatibility
family is also traced in the [cloud/platform audit](audit-cloud-platform.md#diagnostics-inspection-capture-privilege-and-compatibility).

## Topology, lifecycle and persistence findings

Zmux verifies a coherent first slice: Command-T creates an independent tab, tab moves keep
the live session, Command-W preserves siblings, close-other records one grouped history
entry, Shift-Command-T reopens after successful launch, grouped workspace layouts can be
recreated, splits create/collapse correctly, divider changes resize the PTY, and capacity
failures are applied transactionally. Reopened local terminals intentionally create fresh
shells rather than replaying killed processes.

The remaining topology is substantial. Directional pane traversal, numbered selection,
equalize/zoom/automatic layout, focus history, workspace groups and metadata, multiwindow
ownership, cross-window moves, canvas mode, named layouts, versioned session persistence,
previous-launch restore and crash recovery are missing. Closed-pane restore lacks the
original live split anchor, and divider ratios are native view cache state rather than
shared persisted topology.

Undo-close, named session open/save, previous clean launch and crash recovery are separate
contracts. The `surface-resume` alias only manages restart metadata; it does not prove any
of those recovery paths. Likewise, `detach-tab` is an alias for moving a tab to a new
workspace and adds no separate behavior coverage.

Cmux also exposes tmux-compatible synchronization, pane swap/break/join, workspace
next/previous/last, previous-pane, find-window and hook storage. Zmux has related basic
topology actions but none of those compatibility contracts. Cmux intentionally rejects
`popup`, `bind-key`, `unbind-key` and CLI `copy-mode` as unsupported placeholders. That
failure is the observed CLI contract; cmux's native terminal copy mode is a separate
implemented behavior, and the audit does not conflate the two.

Inspection is partial in zmux. `status`, `list-workspaces`, `list-panes` and `read-screen`
expose bounded local state, while cmux separately owns app-wide current-work facts, a full
topology tree, attributed CPU/process/RSS output, app-versus-child memory diagnosis and
surface health. These read paths need stable identity, freshness and unavailable/error
semantics rather than being inferred from generic status counts.

## Native menus, shortcuts and accessibility

Zmux has a compact AppKit menu set and context actions for the implemented tab/workspace
slice. The accepted native smoke exercises Command-T, Command-W and Shift-Command-T via
menu equivalents. It does not establish complete menu validation, disabled states,
physical keyboard behavior, target-window routing or failure presentation.

No native UX row is fully verified. Configurable shortcuts, chords and when clauses are
missing, as is the command palette. Menu and context-menu coverage is partial. Workspace
rows expose some accessibility metadata; tab bars and splitters have implementation paths,
but keyboard and VoiceOver behavior remain unverified. User-facing terminal/topology
strings are not localized.

## Source-catalog namespace reconciliation

`audit-source-catalog.json` contains 217 lexical top-level candidates and 571 advertised
RPC identifiers. These are discovery counts only. Early conditions may be internal paths;
feature flags and DEBUG change availability; an advertised method name does not prove
authorization, handler success or user-visible behavior.

| Source-discovered name | Classification | Feature row | Inventory mapping | Reconciliation |
| --- | --- | --- | --- | --- |
| `detach-tab` | alias | `TOPO-020` | `CMD-063` | Same dispatcher and help as canonical `move-tab-to-new-workspace`. |
| `layout` | canonical | `TOPO-023` | `UX-ACTION-018`; adjacent `CMD-359` | Local saved-layout namespace was absent from the original command ledger. `CMD-359` is `vm layout`, not an alias. Five `layout.*` declarations are lexical discovery. |
| `window` | canonical | `TOPO-019` | `UX-ACTION-006`, `UX-ACTION-007`, `CMD-053`–`CMD-055`, `CMD-084`, `CMD-085` | The namespace itself covers display placement plus offline DEBUG default-display state. Separate verbs/RPCs own broader lifecycle. Thirteen `window.*` declarations therefore do not mean 13 verified window behaviors. |
| `canvas` | canonical | `TOPO-022` | `UX-ACTION-109`–`UX-ACTION-123` | Original command ledger had no canvas row. Twelve `canvas.*` declarations cover discovery only. |
| `workspace-group` | alias | `TOPO-013` | `UX-ACTION-057`, `UX-ACTION-058`, `UX-ACTION-074`–`UX-ACTION-076` | The documented canonical form is `workspace group`; the hyphenated form remains a supported alias. Eighteen `workspace.group.*` declarations do not establish zmux parity. |
| `surface-resume` | alias | `TOPO-024` | no direct row; adjacent `CMD-003`, `CMD-004`, `CMD-382`, `CMD-383` | Canonical `surface resume` stores/reads/clears restart metadata. Three `surface.resume.*` declarations do not establish restore execution. |

No name in this six-item reconciliation is classified as legacy or internal. That does not
generalize to the rest of the source catalog, which contains both kinds.

## Original inventory mapping

The JSON `dependencies` array is the authoritative mapping from each feature to the
original command and UI-action ledgers. Rows omitted below had no exact identifier in
those initial inventories and were added from source behavior.

| Feature row | Original inventory identifiers |
| --- | --- |
| `TERM-001` | `CMD-103`, `CMD-106` |
| `TERM-003` | `CMD-067`, `CMD-068`, `CMD-103` |
| `TERM-004` | `CMD-056`, `CMD-407`, `UX-ACTION-086` |
| `TERM-005` | `CMD-189`, `CMD-190` |
| `TERM-008` | `CMD-106`, `CMD-406` |
| `TERM-009` | `CMD-105` |
| `TERM-010` | `UX-ACTION-080`, `CMD-423` |
| `TERM-011` | `CMD-188`, `CMD-426` |
| `TERM-012` | `UX-ACTION-085` |
| `TERM-015` | `UX-ACTION-141`, `UX-ACTION-143`–`UX-ACTION-146` |
| `TERM-016` | `UX-ACTION-087` |
| `TERM-018` | `CMD-103` |
| `TERM-019` | `UX-ACTION-099`–`UX-ACTION-101` |
| `TERM-021` | `CMD-103` |
| `TERM-022` | `CMD-427` |
| `TERM-023` | `CMD-188`–`CMD-190` |
| `TERM-025` | `CMD-406`, `CMD-408`, `CMD-418`, `CMD-424`–`CMD-428` |
| `TOPO-001` | `UX-ACTION-079`, `CMD-067` |
| `TOPO-002` | `UX-ACTION-044`, `UX-ACTION-045`, `UX-ACTION-054`, `CMD-077` |
| `TOPO-003` | `UX-ACTION-065`, `CMD-073` |
| `TOPO-004` | `UX-ACTION-046`, `UX-ACTION-053`, `CMD-069`, `CMD-071`, `CMD-074` |
| `TOPO-005` | `UX-ACTION-071`, `CMD-068` |
| `TOPO-006` | `UX-ACTION-072` |
| `TOPO-007` | `UX-ACTION-077`, `UX-ACTION-078` |
| `TOPO-009` | `CMD-078` |
| `TOPO-010` | `CMD-021`–`CMD-024` |
| `TOPO-011` | `UX-ACTION-013`, `UX-ACTION-014`, `UX-ACTION-023`, `UX-ACTION-064`, `UX-ACTION-066`, `CMD-007`, `CMD-079`, `CMD-080` |
| `TOPO-012` | `UX-ACTION-059`, `UX-ACTION-060`, `UX-ACTION-067`–`UX-ACTION-069`, `CMD-058`–`CMD-062` |
| `TOPO-013` | `UX-ACTION-057`, `UX-ACTION-058`, `UX-ACTION-074`–`UX-ACTION-076` |
| `TOPO-014` | `UX-ACTION-095`, `UX-ACTION-096`, `CMD-064`, `CMD-066`, `CMD-070` |
| `TOPO-015` | `UX-ACTION-088`–`UX-ACTION-094`, `CMD-065`, `CMD-415`, `CMD-416` |
| `TOPO-016` | `UX-ACTION-103`–`UX-ACTION-106`, `CMD-407` |
| `TOPO-017` | `UX-ACTION-097`, `UX-ACTION-098`, `UX-ACTION-102` |
| `TOPO-018` | `UX-ACTION-061`–`UX-ACTION-063`, `CMD-413`–`CMD-415` |
| `TOPO-019` | `UX-ACTION-006`, `UX-ACTION-007`, `CMD-053`–`CMD-055`, `CMD-084`, `CMD-085` |
| `TOPO-020` | `CMD-057`, `CMD-063`, `CMD-070` |
| `TOPO-021` | `UX-ACTION-004`, `UX-ACTION-008`, `UX-ACTION-012` |
| `TOPO-022` | `UX-ACTION-109`–`UX-ACTION-123` |
| `TOPO-023` | `UX-ACTION-018`; adjacent `CMD-359` |
| `TOPO-024` | `UX-ACTION-020`–`UX-ACTION-022`; adjacent `CMD-003`, `CMD-004`, `CMD-382`, `CMD-383` |
| `TOPO-025` | `CMD-096`–`CMD-098`, `CMD-103` |
| `TOPO-026` | `CMD-409`–`CMD-417`, `CMD-419`–`CMD-423` |
| `TOPO-027` | `CMD-099`–`CMD-103` |
| `NATIVE-001` | `UX-ACTION-004`, `UX-ACTION-146` |
| `NATIVE-002` | `CMD-057`, `CMD-060`, `CMD-063`, `CMD-073` |
| `NATIVE-004` | `UX-ACTION-024`, `UX-ACTION-026`, `UX-ACTION-027` |
| `NATIVE-009` | `CMD-074` |

## Evidence limits

No tests, app builds or live runtime checks were run for this audit. The existing native
receipt verifies source `70cd3b614e0b3f9cf0fcaa0e33bec4c59c228d7a`, not every later change at
the zmux audit SHA. It remains valid evidence for its named subset only. Intel was compiled
and signature-checked there, not runtime tested. Physical keyboard, trackpad, IME,
VoiceOver, resource stress, remote terminals and full window chrome remain open.

Source pointers establish that an owner or path exists at the frozen SHA. They do not
prove runtime availability, option equivalence, focus policy, accessibility, persistence,
error presentation or cleanup. Tests in source are evidence of intended cases; they were
not re-executed in this review. No running cmux instance, repository status or external
state was changed.

## Acceptance themes

Parity closure should proceed through shared terminal/topology actions used by menu,
shortcut, context menu, command palette and CLI entrypoints. Each family needs positive,
invalid-target, lifecycle, focus, failure and cleanup scenarios with stable identities.
Terminal claims need protocol/input/render fixture matrices and bounded overload tests.
Topology claims need multiwindow ownership, deterministic focus and restore behavior,
atomic persistence and explicit fresh-shell versus reattach semantics. Native claims need
packaged macOS interaction, physical input and VoiceOver checks, plus a localization audit.
