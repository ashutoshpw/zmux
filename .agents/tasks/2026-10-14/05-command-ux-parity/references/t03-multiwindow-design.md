# T03 first multiwindow slice

Accepted design, 2026-10-15. The approved singleton model/persistence foundation
is implemented locally; full native multiwindow delivery and acceptance are pending.
Original Zmux MIT
architecture; no cmux GPL/BUSL implementation, tests or assets are imported or
translated. Groups, canvas, window undo-close and complete geometry persistence
remain later T03 slices.

## Evidence and public behavior boundary

The [T03 work package](closure-work-packages.md#t03--multiple-windows-groups-and-canvas-layouts)
and [TOPO-019 audit](audit-terminal-ux.json) require stable window ownership,
cross-window routing, native lifecycle, independent focus and identity-preserving moves.
The existing [T02 navigation](t02-navigation.md) and [S01 persistence](s01-session-persistence.md)
contracts must continue to work in a one-window desktop.

The source review before the foundation found these owners:

- `crates/workspace-core/src/lib.rs`: model v5 has one `focused_workspace` and a
  flat workspace catalog. IDs share a checked allocator and UUID namespace.
- `crates/workspace-core/src/restore.rs`: session document v3 has explicit v1/v2
  readers and no window ownership. Restore preserves logical IDs but starts new
  ordinary shells; immutable tmux owners retain their separate lifecycle.
- `crates/desktop-runtime/src/lib.rs`: one desktop owns all sessions, capacity,
  attention and background operations. Native frames cover only the globally
  selected workspace; attention has one `app_active` flag.
- `apps/macos/Sources/App.swift`: one AppController owns one NSWindow, NativeBridge,
  polling, browser dispatch, notification delivery and terminal/browser registries.
  Native menu targets and BrowserView's owner refer to that controller.
- `apps/macos/Sources/SidebarLayout.swift`: sidebar width/collapse preferences use
  global keys. `Models.swift` derives the visible workspace from global selection.

The pinned cmux public documents at
`3ba653b9e5afb11d0bc578031142950d6136697e` establish the behavior reference:

- [CLI contract](https://github.com/ashutoshpw/cmux/blob/3ba653b9e5afb11d0bc578031142950d6136697e/docs/cli-contract.md):
  list/current/new/focus/close window, workspace moves between windows, stable
  handles and an explicit global window selector.
- [Events contract](https://github.com/ashutoshpw/cmux/blob/3ba653b9e5afb11d0bc578031142950d6136697e/docs/events.md):
  registered main windows, app-level focus requests and observed AppKit key/unkey
  transitions are distinct facts.
- [Public API plan](https://github.com/ashutoshpw/cmux/blob/3ba653b9e5afb11d0bc578031142950d6136697e/docs/agent-browser-port-spec.md):
  window lifecycle and workspace move-to-window share stable object routing.

The user-supplied move command uses `--workspace HANDLE --window DESTINATION`.
The future grammar below preserves that contract; remaining failure semantics are
explicit Zmux design decisions. No upstream implementation source was needed.

## Approved singleton foundation

Model v6 now adds `WindowId`, canonical membership/selection and a derived legacy
`focused_workspace`. `MAX_WINDOWS = 1` applies to every persisted ingress and
every successful mutation. First workspace creation reserves one window after
the original four object IDs, preserving those initial identities. Closing every
workspace retains the same empty singleton. Reopen, moves, focus, navigation and
failed restore cleanup share these ownership paths. A rejected action commits
neither topology, revision nor identifiers.

Session v4 stores the new data in one versioned field:

```text
window_layout: {
  windows: [{ id, workspace_ids: [...], selected_workspace }],
  focused_window
}
```

Persisted v4 requires exactly one checked window, including an empty desktop;
only an ephemeral never-started store may have none. Validation rejects missing
or null layout, duplicate/missing/unknown memberships, inconsistent selection or
legacy focus, cross-kind ID collisions and additional windows before restore.
Canonical membership order projects into the flat native singleton catalog.
V1/v2/v3 require the layout field to be absent and migrate to one window. The
filesystem worker durably reserves its ID in the same block as fresh session
IDs, above both the imported floor and prior ledger generations, before returning
any prepared model/effects. Empty legacy desktops also reserve a window.
V4 preserves the saved window identity while session IDs remain fresh. Existing
workspace/pane/surface IDs and namespace stay stable; imports never rewrite source.

This foundation adds no new-window actions/CLI, target-window routing or native
coordinator. The proposed two-window cap below applies only to a future native
implementation. One existing AppKit host still owns presentation.

Local Linux verification, 2026-10-15:

- Production `windows` tests first failed because canonical ownership was absent;
  both pass after implementation. The empty v4 ownership rejection then failed
  on accepted zero-window input and passes after persisted-singleton validation.
- `cargo test -p zmux-workspace-core -p zmux-desktop-runtime --locked --target-dir /tmp/zmux-t03-foundation-target`
  passed all 131 tests: 46 core and 85 desktop, including real persistence worker
  restart/ledger checks and existing runtime PTY restore/cleanup suites.
- `cargo clippy -p zmux-workspace-core -p zmux-desktop-runtime --all-targets --locked --target-dir /tmp/zmux-t03-foundation-target -- -D warnings`
  passed. Owned Rust files were formatted and `git diff --check` passed.
- Native model-version guard v6 is coordinated with the browser implementation
  owner. Universal native compilation and packaged execution of this model change
  remain independent gates; the two-window native acceptance list below remains
  wholly pending.

## Initial schema and shared actions

Use model v6 with a new `WindowId` from the existing global allocator. Window UUIDs
derive from the existing namespace and the `window` kind; workspace, pane, surface
and session IDs and existing UUID derivations do not change.

Keep the flat workspace catalog. Add a canonical ordered window collection:

```text
Window { id: WindowId, workspace_ids: [WorkspaceId], selected_workspace: WorkspaceId? }
Snapshot { windows: [Window], focused_window: WindowId?, workspaces: [Workspace], ... }
```

Membership is authoritative: every workspace occurs exactly once across windows;
every selected workspace belongs to its window. Do not store a second mutable
workspace owner field. Empty windows are allowed and select no workspace. Retain
`focused_workspace` as a derived compatibility projection of the focused window's
selection, never an independently writable global selection.

The first native acceptance slice can cap main windows at two. This is a bounded
initial feature policy, not a measured memory budget or full upstream parity.
Keep eight terminals, four browsers, 32 workspaces and existing geometry/history
limits global; creating another window must not create another allowance.

| Shared action | Concrete behavior |
| --- | --- |
| `CreateWindow { focus }` | Reserve all required IDs and global terminal/workspace/window capacity before committing one window with one starter workspace and terminal. A failed terminal remains inspectable through existing launch-failure semantics. |
| `FocusWindow { window_id }` | Select the window's remembered workspace/pane/surface and request native activation. Track the request separately from observed key-window state. |
| `CloseWindow { window_id }` | For a non-last main window, close only its owned workspaces/sessions through shared close paths, then reconcile the native host. Last-main-window close follows the save/quit rule below. |
| `MoveWorkspaceToWindow { workspace_id, destination_window_id, index, focus }` | Validate both live windows and source membership at one revision, then move membership/order without any session start/stop effects. |

Native controls, menus, shortcuts and socket commands enter these same actions.
Existing workspace creation, selection and navigation receive resolved window
ownership. A missing/closing native destination, stale revision, capacity failure
or invalid membership rejects before authoritative topology/IDs change.

## Exact move and no-focus rules

Moving a workspace preserves its ID, split tree, selected surfaces, session IDs,
PTY PIDs, SSH/tmux owners, browser profile/storage, WebKit host/document generation,
metadata, pending transfers and notification attribution. Move existing views
between hosts and rebind their UI callbacks; do not restart shells or recreate
BrowserView to implement placement.

For `focus: false`:

1. Keep the source native window key and do not activate the app or destination.
   If a different window was key, preserve that window instead.
2. Preserve destination selection when it already has a selected workspace. An
   empty destination selects the moved workspace without becoming key.
3. If the moved workspace was selected in the source, select the remaining entry
   at its former index, otherwise the preceding entry; an empty source selects
   nothing and renders an explicit empty state. Do not launch a replacement shell.
4. Recompute the global compatibility projection from the focused source window.
   Leaving it pointing at the moved workspace would silently route foreground
   operations to the destination and violate no-focus behavior.

For `focus: true`, select the moved workspace in the destination and submit an
explicit destination focus request. Background topology reconciliation alone must
never invoke activation.

Partition existing bounded focus/workspace navigation histories by owning window.
Prune source history entries for moved/closed content; a no-focus move does not
insert a destination focus event. Existing tmux-compatible next/previous/last-window
aliases continue to navigate workspaces within the resolved window.

## Native coordinator and acknowledgement boundary

Extract one application coordinator from AppController. It owns the single
NativeBridge/Desktop, control socket, bounded command/polling/browser event loops,
notification delivery, image-paste operations, profile data stores and global
terminal/browser view registries. Introduce MainWindowHost objects indexed by
stable WindowId. Each host owns its NSWindow, sidebar/header/layout/responder state
and a projection for that window's selected workspace. No host starts or shuts
down its own Desktop.

BrowserView must use an injected shared command/observation owner rather than a
per-window AppController identity. Shared view registries retain hidden browsers
and in-flight operations across moves. Profile data stores and their capacity stay
app-wide. Remove a view from the old hierarchy before attaching it to the new one;
there is exactly one live view/host attachment per surface/session.

Publish the union of all windows' selected workspace frames, while retaining
app-wide control inspection frames. Each host receives only its owned projection.
Rebind terminal focus/input/resize and browser toolbar callbacks to the new owning
window during a move; existing session/document identities remain intact.

Add bounded, epoch-scoped native window requests and receipts with states that
distinguish admission from actual native materialization/closure. Native callbacks
carry WindowId plus host generation; late events from closed or restored hosts
cannot affect a replacement. A logical record alone does not complete new-window.
Focus-request completion means the native request was performed, not that the OS
necessarily granted keyboard focus. Report observed key-main-window identity
separately and prove key-window changes in controlled native acceptance.

AppKit constructs/reparents/closes views on MainActor. Rust admits and commits
topology only. PTY startup/cleanup, profile work, persistence and socket waits stay
on existing workers/actors. Neither the UI thread nor the core executor waits for
the other to finish a window operation. UI close delegates enqueue the shared
request; authorized native closure is fenced against recursive re-admission.

Menus resolve an owned main-window context through the responder/coordinator path,
not a global controller target. Auxiliary cloud/profile/settings windows are not
workspace windows. Their key status does not masquerade as terminal visibility.

## Caller routing and attention

Add window handles and `TargetScope.window`. Resolve explicit window/workspace/
pane/surface constraints against one immutable revision; mismatched ownership
fails instead of borrowing foreground selection. Include owning window IDs in
target catalogs and mutation results without dummy workspace IDs for window roots.

Proposed public Zmux forms are list-windows, current-window, new-window
`[--no-focus]`, focus-window/close-window `--window HANDLE`, and
move-workspace-to-window `--workspace HANDLE --window DESTINATION [--no-focus]`.
For this command, the explicit workspace identifies the source and command-specific
`--window` identifies the destination; it must not also constrain source ownership.
Other commands retain `--window` as a context constraint. Resolve and validate the
source workspace and destination window at one revision, rejecting ambiguous or
conflicting selectors. Do not advertise an unimplemented leaf or option.
Help remains offline, and provider arguments after `--` retain their existing
passthrough behavior. Preserve existing global inventory reads and add explicit
window filters; implicit mutations resolve a caller or focused main window.

Exported ambient window context is only a hint. Persistent shells retain old
environment values after a move: first resolve their surviving surface/workspace
and derive its current window. An explicit context `--window` is a hard constraint;
the move command's destination has the distinct semantics above.
Managed tmux callers continue immutable owner -> exact surface -> workspace ->
current window resolution; ambiguity fails. A pure window move requires no tmux
registry owner rebinding because workspace/surface identities are unchanged.

Replace the single attention-active flag with app activity and observed key-main-
window identity. Preserve the current focused-pane/selected-surface read policy
inside that window. Notifications keep their stable workspace/surface/session
attribution; opening/replying revalidates it and derives the current owning window.
Do not embed a stale authoritative window in the OS notification payload. One
coordinator delivers notifications, and only a validated navigation result requests
activation of the exact host. Moving content does not mark its notifications read.

## Persistence and last-close semantics

Session document v4 records window IDs, ordered workspace membership, per-window
selection and focused-window selection. Keep explicit v1/v2/v3 readers. Legacy
documents synthesize one window using checked allocator reservation above existing
IDs/floors, preserving all existing logical identities and namespace. New-window
creation needs five IDs, so update allocator headroom checks accordingly. Reject
duplicate/missing membership, invalid selections, exhausted IDs and capacity excess
before restore changes authoritative state.

Per-window sidebar width/collapse keys use namespace plus Window UUID. The legacy
single window inherits existing global defaults. Native geometry and split-ratio
persistence remain separately named later gates; this slice must not claim them.

Preserve quit-after-last-main-window behavior through the existing asynchronous
save/shutdown path. Last close requests termination and saves the recoverable
pre-close model; do not first erase the last window/workspaces and autosave an
empty desktop over the recovery document. Route Cmd-Q, last-main-window close and
CLI last close through that same coordinator path, including auxiliary-window
handling. Persistence failure remains explicit; teardown must not offer a false
cancel/retry into already stopped sessions. A lost close acknowledgement is an
unknown outcome and must not cause automatic mutation replay.

Closing another window stops only its owned sessions and persists the remaining
desktop. Existing workspace undo-close needs a validated live original window or
an explicit deterministic requesting-window fallback. Full window undo-close,
group recovery and crash-window geometry restoration are not delivered by this slice.

## Acceptance before delivery

- Real two-window create/list/current/focus/close through CLI and native controls;
  offline help, exact returned identities and honest pending/native states.
- Both windows' selected terminals continue rendering and resizing. Key/main
  window and first-responder observations match input delivery; auxiliary windows
  and background reconciliation do not steal focus.
- No-focus moves preserve the key window and destination selection, including
  moving the source's last/selected workspace to an empty destination.
- Moves retain exact PTY PID, tmux owner, WebKit host/document generation and private
  cookies; pending automation/transfers/replies stay attributable after moves.
- Explicit cross-window mismatches, stale ambient window hints, ambiguous tmux
  callers, stale revisions and late native host callbacks fail safely.
- Window/workspace/session capacity failures are atomic and global. Closing a
  background window preserves unrelated sessions and native focus.
- Per-window sidebar state, menu/responder ownership and independent navigation
  histories work; notification open reaches the moved target's current window.
- Legacy v1/v2/v3 migration and v4 round-trip preserve logical identities; invalid
  memberships fail before restore. Last-close/restart retains the pre-close layout,
  including persistence failure and auxiliary-window cases.
- Portable owner-boundary behavioral tests precede repairs. Universal compilation
  and executed packaged macOS journeys are required separately. Physical keyboard,
  IME, VoiceOver and measured multiwindow resource behavior remain explicit gates.

The singleton foundation is not full T03/V01 acceptance or a production resource
budget claim. Native browser/profile/tmux owners must integrate with the future
coordinator before additional logical windows can be exposed.

## Parent integration verification

Full `scripts/check` passed in isolated snapshot
`/tmp/zmux-t03-parent-verify-69b6klb2` using fresh private Cargo target
`/tmp/zmux-t03-parent-target-69b6klb2`; log
`/tmp/zmux-t03-parent-clean-check.log`. This includes workspace tests, strict Clippy,
notice inventory and real C ABI checks. Parent source review confirmed transactional
window allocation, durable legacy reservation above ledger generations and singleton
selection projection. Malformed membership cases were strengthened to reach their
own guards. Native model-version admission advances to 6 with this schema.

A first run reused a sequential Cargo target across snapshots with preserved
source mtimes and consumed stale pre-change library metadata. The fresh target
passed without changing production source; per-snapshot targets are required.
Hosted native execution of model 6 remains pending.

## Native coordinator extraction foundation

Original MIT extraction implemented locally, 2026-10-15; native compilation and
packaged execution remain pending. No additional logical/native window action,
CLI command, capacity or multiwindow capability is exposed.

`ApplicationCoordinator` now owns the sole production `NativeBridge`, bounded
command/polling/browser-event loops, notification delivery, image-paste operations,
downloads, profile data stores and global terminal/browser registries. Startup is
one-shot; shutdown callers share one retained task, and startup cannot launch loops
after shutdown interrupts its bridge await. `MainWindowHost` owns the existing
NSWindow, sidebar/header/layout, menu targets and responder state. It receives its
coordinator and never starts or shuts down a Desktop.

Native decoding now includes canonical window records. `MainWindowProjection`
validates the singleton membership and selection before native resource changes,
then constructs the presenter's read projection in canonical membership order.
The coordinator retains the full snapshot and registers the presenter by identity
namespace plus canonical WindowId. A never-started empty owner may be unbound;
additional windows or inconsistent ownership fail explicitly. Layout runs before
the shared view-content update and responder reconciliation, preserving first-frame
and focus ordering.

Browser views use the injected application-level `BrowserCommandOwner` rather than
the window presenter. Downloads use `BrowserDownloadOwner`; destination sheets
resolve their immutable origin surface and current canonical workspace/window to
the registered presenter, without choosing a focused window. Existing download
tokens, physical-credit retention, conversion delegates, staging/file publication
and cancellation state transitions are unchanged. Terminal view creation remains
in the shared registry; presenter callbacks keep their existing explicit pane,
surface and session routes.

Validation: `CARGO_TARGET_DIR=/tmp/zmux-t03-coordinator-target scripts/check` passed,
including workspace tests, strict Clippy, notice inventory and actual C ABI checks;
log `/tmp/zmux-t03-coordinator-check.log`. Scoped whitespace checks passed. Swift
and the macOS SDK are unavailable on this Linux host, so these portable checks do
not establish Swift compilation, native resource lifetime or UI behavior. Universal
native compilation and the existing packaged journeys, especially browser profiles,
downloads, clipboard images, notifications and local tmux, remain required gates.

Two-window lifecycle/admission receipts, per-window sidebar preferences, menu/context
routing, observed key-window attention, no-focus moves and last-main-window close
with auxiliary windows remain future vertical work. This extraction is a singleton
foundation and does not satisfy the multiwindow acceptance list above.

Parent independent source review and full `scripts/check` passed against frozen
snapshot `/tmp/zmux-coordinator-parent-verify-dnkj0s2o` with a fresh private Cargo
target; log `/tmp/zmux-coordinator-parent-check.log`. Native compilation and packaged
smoke remain required before native acceptance.
