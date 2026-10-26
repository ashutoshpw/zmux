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

## Two-window vertical slice, 2026-10-15

This original MIT implementation supersedes the singleton capacity restriction
above. It is a bounded two-main-window slice, not complete command parity or
native acceptance. One Desktop/coordinator continues to own every PTY, browser,
download, notification and control socket. Canonical window membership, selected
workspace, closed-item origin and navigation history live in Rust. A focused empty
window is valid even while its sibling has workspaces; v4 persistence preserves it.

The public CLI now exposes list/current/new/focus/close window operations and hard
`--window` constraints on object routes. Window roots resolve without requiring a
workspace/pane/surface. Valid ambient window hints apply only without an object
caller; surface/workspace callers derive their canonical current owner. Object
receipts, pane/surface lists and read-screen include owning window IDs/UUIDs.
Global list-panes optimization is unavailable with explicit or ambient window scope.

Native mutation admission returns an epoch, operation UUID and exact window
identity. CLI waits under one absolute deadline for that exact native receipt and
never replays a mutation. The runtime retains at most two physical host credits
and 32 operation receipts. A 30-second native timeout marks the receipt failed or
unknown without freeing a possibly live host. Exact close acknowledgement releases
that window's credit and drains older pending operations; late opens cannot revive
it. Failed materialization stays visible in list-windows with an explicit close or
quit/reopen recovery instruction. No automatic materialization retry occurs.

AppKit registers presenters by namespace/window identity and uses trusted native
host tokens. Known pre-effect failures send explicit Failed events; uncertain ABI
acknowledgements retain the operation without replay. Global detach precedes every
new attachment, frames cover the union of every window's selected terminal tabs,
and each host retains its own responder/menu/sidebar context. No-focus creation
orders the sibling below the current key window. Attention visibility requires an
observed trusted key-main-window owner as well as app activity. Native window close
and CLI close share the runtime route. Last-main-window close retains its logical
layout and enters the ordinary application termination/save path, including while
auxiliary windows are open. Shutdown still reaches Desktop save/session cleanup if
native download cleanup fails.

Portable evidence:

- The initial core regression failed on unsupported `create_window`, then passed
  with atomic/no-focus creation and ownership-preserving close; exact red command
  and log are `/tmp/zmux-t03-two-window-red.log` (an earlier zero-test invocation
  is not red evidence).
- Runtime owner tests use two real PTYs, exercise empty-key-window frame visibility,
  forged/stale host tokens, exact acknowledgement/capacity, and 40 consecutive
  close-before-open-ack generations with bounded retention.
- Real filesystem/PTY persistence tests round-trip a focused empty second window,
  retain the terminal owner's topology on last-close acknowledgement, inspect the
  actual saved document and reopen a fresh runtime shell.
- The actual C ABI test proves response-buffer capacity precedes host mutation and
  rejects stale host generations; native-event forging is unavailable over socket
  commands.
- Full `scripts/check` passed with private target
  `/tmp/zmux-t03-two-window-check-target`; log
  `/tmp/zmux-t03-two-window-check.log`. This covers workspace tests, strict Clippy,
  notice inventory and real C ABI checks. CLI failed-open recovery tests are part
  of this combined run.

Native source and acceptance instrumentation are present but have not compiled or
executed on this Linux host. `scripts/check-native-windows` launches the packaged
app through Launch Services with a fresh private persistent profile and socket.
Its close phase checks native no-focus/responder retention, independent resize,
window-targeted AppKit key events, native sibling close, empty-window root routing,
local workspace recreation and exact CLI closure. It writes preparation evidence
before last-main-window close with an auxiliary window open. The external runner
then inspects the actual post-exit v4 session file, requires a final write after
preparation, relaunches the same profile and verifies exact logical identity with a
fresh native terminal and input. Failures retain their logs/profile, and timeouts
do not trigger a second app launch or remove possibly live storage. The runner is
invoked after packaged validation and before save-panel driver setup and the broad
smoke in `scripts/check-macos-app`, so browser/Accessibility failures cannot prevent
independent window evidence. A window failure stops execution before another app
launch.

Universal Swift compilation and successful packaged execution of both existing
smoke and this lifecycle journey remain required before native acceptance. The
native close-before-open race is not claimed by that journey; its deterministic
40-generation proof is at the actual portable runtime owner. Physical keyboard,
IME, VoiceOver, measured two-window resource behavior, external-app focus and
injected native persistence failure remain separate acceptance gates.

Parent independent combined snapshot passed full `scripts/check` with a fresh
Cargo target (`/tmp/zmux-t03-parent-check.log`). Independent native source review
found no blocking ownership/focus/lifecycle issue. No native execution is claimed.
The combined check includes the existing socket deadline fixture repair: Darwin
ENOTCONN is an expected disconnected-peer write result after timeout; ordinary
success, absolute deadline and no mutation replay assertions remain intact. The
first hosted evidence for this error is macOS run38781114605 at3438565.

## Workspace move vertical slice, 2026-10-15

Original MIT move implementation is present locally; native compilation/execution
remains pending. It keeps the two-window bound and does not deliver groups, canvas,
workspace undo-close or portable geometry export.

`move-workspace-to-window --workspace HANDLE --window DESTINATION` now separates
the source selector from its destination. Both resolve on one executor snapshot;
`--expect-revision N` optionally fences a previously observed catalog. The CLI
now defaults to no focus, matching the pinned reference CLI. `--focus true|false`
is an explicit extension; `--no-focus` is an explicit false alias and conflicts
with `--focus`. Native menu actions explicitly choose their desired focus mode.
Offline help and the local command contract describe the same grammar.

The core `MoveWorkspaceToWindow` action moves canonical ordered membership only,
retaining every workspace, pane, surface, session and namespace identity. It emits
no start/stop effects and consumes no identifiers. The selected source falls back
to the next entry at the removed index or its predecessor, with no replacement
shell in an empty source. No-focus preserves the logical/key window and populated
destination selection; an empty destination selects the moved workspace. Histories
prune moved content without inserting a destination focus event. Existing saved v4
window membership/order/selection needs no new document format.

The runtime preflights a live registered native destination for every entrypoint
before topology commits. A focus move reserves a fresh, exact native focus operation
on a candidate WindowRuntime before assigning the candidate model. Capacity or
host-readiness failure therefore leaves topology unchanged. Move JSON reports
`state: topology_committed`, exact workspace/source/destination IDs and UUIDs, model
revision and an optional separate `native_focus` receipt. No-focus admission does
not claim native projection is already drawn. A focus failure/timeout reports that
topology already committed; neither CLI nor native code replays or rolls it back.
CLI verifies the focus receipt belongs to the committed destination before waiting.

Native menu targets carry stable workspace/window UUIDs through the queue, so stale
numeric IDs cannot alias a replacement namespace. The coordinator retains existing
TerminalView and BrowserView instances and detaches all changed hierarchies before
attaching any new one. Each new host hierarchy rebinds terminal callbacks; source
content releases its responder before views leave that NSWindow. Browser hosts,
document generations, profile data stores and pending operations stay app-owned.
Notification activation and new destination sheets derive current membership from
the immutable originating workspace/surface rather than inherited window hints.

Split ratios now belong to native workspace preferences, keyed by identity namespace
and Rust-published stable workspace UUID. Divider changes persist bounded finite
ratios; reconciliation prunes removed workspaces and split keys, and a namespace
replacement removes the old active namespace's preferences. Ratios survive moves,
closing the former host and app restart. This is native preference persistence,
not portable session-export geometry; full T03 geometry remains open. Sidebar
preferences continue to belong to each window.

Same-window behavior remains an explicit unresolved parity item. Zmux's public
move with no insertion index is a bounded selection-preserving no-op, including
`--focus true`; it may advance the normal successful-action revision but emits no
native focus request. Read-only inspection at pinned reference `3ba653b9` found
entrypoint differences: the native AppDelegate helper focuses/selects the workspace
when asked, while the V2 CLI path detaches/reattaches without a same-manager guard
and defaults `focus` to false. Public docs only promise moving to a different
window. No reference implementation was copied or translated. Documenting this
bounded decision does not close same-window parity or full T03.

Owner-boundary validation:

- The new core behavioral regression failed on unknown `move_workspace_to_window`
  before the action existed; `/tmp/zmux-t03-move-red.log`. It now covers selected
  and background source workspaces, populated/empty destination, logical source/
  destination focus, requested focus/no-focus, identity preservation, zero effects
  and atomic invalid-destination rejection.
- Real two-PTY runtime coverage rejects an unmaterialized destination before any
  change, preserves PIDs/session IDs, accepts input from a surviving caller with a
  stale window hint, reserves 32 distinct focus requests and rejects the next move
  atomically when physical request capacity is full.
- The actual CLI/socket journey covers default no-focus, distinct destination
  grammar, inherited stale-window context, native focus success/failure, committed
  topology after focus failure, stale revisions, invalid/conflicting grammar and
  bounded same-window no-op behavior.
- Actual filesystem/PTY save/restart coverage preserves moved membership and its
  focused empty source, stable logical identities and fresh restored sessions.

`WindowLifecycleSmoke` now prepares a terminal/browser split, moves the existing
workspace to the other window without focus, activates a notification at its new
owner, and moves it back through the native shared action. It checks the retained
PTY PID, terminal/browser objects, WebKit host/document/data-store identity, a
browser operation started before movement, metadata, input after former-host close,
and a 0.63 split ratio both across hosts and after restart. The external runner
inspects the real saved split topology. These are pending native acceptance checks,
not executed evidence on Linux. Physical keyboard/IME/VoiceOver, download sheets
already open at movement, portable geometry export, same-window parity, groups and
canvas remain separate gates.

The initial move `scripts/check` passed 253 tests, strict Clippy, notice inventory
and real C ABI checks with private target `/tmp/zmux-t03-move-check-target`; log
`/tmp/zmux-t03-move-check.log`. Final parent integration verification is still
required after CLI default alignment, native diagnostics and the separately owned
browser-zoom slice. The latest focused CLI window journey passes all five tests
(`/tmp/zmux-t03-move-cli-final.log`), including default no-focus and explicit focus.
No local Swift/macOS build is available.

Earlier hosted T03 `16cf61a` timed out after 150 seconds in the close phase without
a preparation receipt or stdout/stderr. Its cause remains unconfirmed; it does not
establish native window or move acceptance. Gated diagnostics now identify startup
before coordinator/presenter construction, bridge startup, first snapshot, native
requests, fixture wait sites and shutdown/save stages. Stderr trace count and the
private 64 KiB trace file are bounded. The runner copies bounded phase logs, trace,
preparation evidence and a read-only process snapshot into retained `dist/native-`
logs on timeout/failure. The deadline stays 150 seconds; no second app is launched,
no process is signaled, and potentially live profile storage remains untouched.

Final integrated portable verification: `scripts/check` passes in fresh snapshot
`/tmp/zmux-move-zoom-integrated-jfol05gu` (255 Rust tests, formatting, strict
Clippy, dependency notices and C ABI). All 35 captured production/native/script
inputs match the working source after verification; log
`/tmp/zmux-move-zoom-integrated-check.log`. Swift compilation and actual native
execution remain separate hosted gates. The preceding `16cf61a` universal app
compiled but its window close fixture timed out; new bounded phase traces retain
evidence without another app launch or extending the timeout.

## Canonical workspace ordering slice (2026-10-15)

CMD-058 (`reorder-workspace`) and CMD-059 (`reorder-workspaces`) now have bounded
local counterparts. This is original MIT Zmux code. The read-only behavioral
reference is cmux `3ba653b9e5afb11d0bc578031142950d6136697e`: CLI help/parser in
`CLI/cmux.swift` around 11974–12094 and 21943–21983; workspace reorder routing in
`Sources/TerminalController+ControlWorkspaceContext.swift` around 145–235;
placement behavior in `Packages/macOS/CmuxWorkspaces/Sources/CmuxWorkspaces/Coordinators/WorkspaceReorderCoordinator.swift`
around 220–243; bulk behavior in that package's `Reorder/WorkspaceReorderPlanner.swift`;
preview/result behavior in `Packages/macOS/CmuxControlSocket/Sources/CmuxControlSocket/Coordinator/Workspace/ControlCommandCoordinator+Workspace.swift`
around 359–549. No reference implementation, tests or assets were imported or translated.

The local CLI accepts a subject through `--workspace HANDLE` or one positional
handle, exactly one of signed `--index`, `--before`/`--before-workspace`, or
`--after`/`--after-workspace`, optional hard `--window`, `--dry-run`, and the local
optimistic revision guard `--expect-revision`. Placement indices are final,
zero-based and clamp to the legal window range. Relative placement accounts for
removing the source from the original order. The observed reference edge is
intentional: before-self stays in place, while after-self advances one final
position, clamped at the end. The primary behavioral table fixes these independent
examples explicitly.

Bulk `--order HANDLE,HANDLE,...` trims entries and rejects empty lists/entries,
duplicate resolved workspaces, unknown targets and cross-window membership before
commit. Requested IDs form a leading prefix; unmentioned IDs retain their relative
order. Zmux currently has only the ungrouped/unpinned partition. Pin/group ordering,
full reference text/errors and complete wire/output compatibility remain unresolved;
the frozen mapping explicitly says compatibility unverified.

Rust owns one pure order planner, shared by executor preview and the core mutation.
The action updates only `Window.workspace_ids` and model revision, with no effects
or identifier allocation. Workspace/pane/surface selections, focused window,
existing sessions, metadata, histories and split geometry remain intact. Both
symbolic resolution and commit occur on the workspace executor at one model
revision. Plan rows include requested unchanged entries; applied events include
only requested changed rows and remain empty for dry-run. Dry-run leaves model
revision/allocator/history/persistence unchanged and is admitted by read-only
socket policy; apply requires mutation access. No native focus/materialization
receipt is fabricated by this logical ordering operation.

Reorder-specific `index:N` targets remain one-based, but resolve from the chosen
window's canonical membership order, including after earlier moves/reorders.
Other legacy commands retain their existing selector behavior. An explicit window
fences every subject/anchor/batch item. Without it, the first stable subject derives
its owning window; an index subject uses the surviving caller's objects, then
its window context or focused window. Conflicting or stale caller object claims
reject rather than borrowing another window. Live caller objects override a stale
inherited window hint after movement.

Native row menus expose Move Up/Down/Top/Bottom with stable workspace/window UUIDs
and the menu's observed model revision. They enter the same order executor and
planner as CLI. Boundary items disable, and a stale menu rejects visibly. The
separate `WorkspaceOrderSmoke` fixture drives the actual row menu and bundled CLI,
then checks real sidebar order, unchanged key window/content responder, live PTY
PIDs/native terminal objects, and WebKit objects/host IDs/document generations. It
restores the original order before continuing the broad smoke. Its required report
key is `workspace_order`. This native journey is authored but unexecuted locally;
Linux has no Swift/macOS SDK. Window lifecycle diagnostics and the two-window cap
were not changed.

Test authoring gate: the core tests protect order/identity/atomicity against wrong
relative-index math, unstable unlisted order and partial invalid commits; existing
move tests do not execute reorder. Socket tests uniquely protect parser routing,
read-only admission, executor revision guards, canonical scoped indices, stale
caller ownership and preview/apply receipts. The real filesystem/PTY test protects
save/restart order and session continuity, which a core-only snapshot cannot prove.
The native journey reaches real AppKit/WebKit presentation that portable Rust
cannot establish. None adds a test-only production seam.

Scoped verification (private Cargo target `/tmp/zmux-t03-order-target`):

- New core behavior first failed on unknown `reorder_workspaces` before production
  implementation (`/tmp/zmux-t03-order-red.log`); the same focused command now
  passes both tests (`/tmp/zmux-t03-order-green.log`), including after-self and
  cross-window/unknown/duplicate/empty rejection with unchanged full model/allocator.
- `cargo test -p zmux --test windows reorder_cli --locked`: 2 actual CLI/socket
  journeys passed (`/tmp/zmux-t03-order-cli-tests.log`).
- `cargo test -p zmux-desktop-runtime --test session_persistence reordered_membership`:
  actual save/restart and three real PTYs passed
  (`/tmp/zmux-t03-order-persistence.log`). Restored sessions are fresh; logical
  membership and selection remain exact.
- `cargo clippy -p zmux-workspace-core -p zmux-desktop-runtime -p zmux --all-targets --locked -- -D warnings`
  passed (`/tmp/zmux-t03-order-clippy.log`). Owned Rust files were formatted;
  `git diff --check` and native runner Python syntax passed.
- Executable `zmux commands --check` passed with compatibility still false
  (`/tmp/zmux-t03-order-catalog.log`). The parent owns final integrated checks and
  hosted/native validation; no predecessor full suite was rerun for this slice.

Integrated verification exposed an existing cwd-output fixture assumption: a
correct restored `pwd` could share a terminal line with its interactive prompt.
The fixture now deliberately sets a real prompt with tty echo disabled, waits for
that prompt, then frames actual `pwd` output between separate begin/end lines.
It still requires the exact expected cwd as the complete middle line. This setup
reproduced the old predicate's timeout (`/tmp/zmux-t03-order-cwd-red.log`); the
repaired full 12-test persistence suite passes
(`/tmp/zmux-t03-order-persistence-final.log`). No persistence behavior changed.

## Numbered workspace selection slice (2026-10-15)

TOPO-011 / UX-ACTION-064 now has a bounded default numbered path. Read-only
behavioral references at cmux `3ba653b9e5afb11d0bc578031142950d6136697e` are
`Sources/TabManager+NumberedWorkspaceSelection.swift`,
`Sources/KeyboardShortcutSettings.swift:687`, `Sources/AppDelegate.swift:16335`,
and numbered menu entries in `Sources/cmuxApp.swift:1299`. The independent contract
is Command-1 through Command-8 for those ordinary sidebar rows; Command-9 selects
the last ordinary row, including when there are fewer or more than nine rows.
Matched digits are consumed even when the requested row is unavailable. Reference
group-header/collapsed-child filtering is not claimed: Zmux has no group model.
No source implementation or tests were copied/imported/translated.

The Rust snapshot maps a validated digit against the exact window's canonical
membership. A targeted operation resolves the hard UUID window scope on the
executor and uses the existing `SelectWorkspace` action for an available row.
Unavailable rows, including empty windows while another window is focused, skip
the action entirely: no model revision, selection, history or session mutation.
Invalid digits reject. Remembered pane/tab selection and existing sessions remain
owned by their workspaces. No new CLI grammar or global-index behavior was added.

Native workspace menu entries and the production `SidebarFocusWindow` key-
equivalent handler share UUID-scoped selection dispatch. The window handler
consumes exact Command-digit defaults before terminal/browser fallback even if
native startup has not supplied a selectable row. Menu entries store their digit and use the same host route as key events. That
route requires the actual key owner and current namespace, then submits the
current stable window UUID; a stale menu cannot activate another main window. Control/Option/Shift combinations and other digits retain
their ordinary event routing. Shortcut rebinding/chords, group visibility and a
Go To Workspace palette remain unresolved separate work.

`WorkspaceNumberSmoke` drives actual `NSWindow.performKeyEquivalent` with synthetic
Command-digit events and the real numbered menu. It switches between existing
terminal/WebKit responders, verifies remembered panes/tabs and native view/host/
document identities, checks live PID continuity, exercises unavailable slots from
both content kinds, reverses canonical order and checks numbered/last semantics,
then restores order and selection. The broad native report requires
`workspace_number`. This fixture has not been compiled or executed on this Linux
host; physical keyboard/IME and cross-window native acceptance remain separate
hosted gates. Parent startup/menu diagnostic work is independently owned.

Test authoring gate: the primary pure mapping table guards independent digit
examples for counts 0/1/2/8/9/12 after reversal, wrong-window rows and invalid digits.
Existing direction/history navigation tests cannot catch a 9-as-index regression.
The separate real-PTY executor test protects unavailable-row no-mutation behavior,
UUID window resolution, the existing selection action and unchanged sessions/
remembered selections; it also checks an explicit empty source while a different
window is focused. Native event/menu consumption cannot be established in Rust.
No test-only production seam was introduced.

Local scoped verification uses `/tmp/zmux-t03-number-target`:

- New core API test first failed because the numbered owner did not exist
  (`/tmp/zmux-t03-number-red.log`); the table passes after implementation
  (`/tmp/zmux-t03-number-green.log`).
- `cargo test -p zmux-desktop-runtime --test windows numbered_workspace_requests --locked`
  passes the real-PTY/executor journey (`/tmp/zmux-t03-number-runtime.log`).
- `cargo check -p zmux --locked` and strict Clippy for core/runtime all targets pass
  (`/tmp/zmux-t03-number-check.log`, `/tmp/zmux-t03-number-clippy.log`).
  Owned Rust formatting, `git diff --check` and native runner Python syntax pass.
  Parent owns integrated checks, delivery and native execution.


### Remembered native browser focus repair

Independent numbered review exposed a shared owner gap: layout rebuilding always
called `focusAddress`, and the completion owner skipped browser content focus
when the layout changed. That contradicted the intended return to a previously
document-focused loaded browser. Read-only comparison at the same pinned cmux
source found that `Sources/Panels/BrowserPanel.swift:2432` defaults to document
focus, `:8172–8253` captures/restores document versus address/find intent, and
`:5090` keeps blank/unloaded content from taking address focus. This is behavioral
comparison only; the repair remains original Zmux code.

BrowserView now remembers its native address/document destination. It captures
only an actual AppKit responder owned by the host immediately before detach,
including its real address field editor and selection. It preserves the previous
WebKit responder while that responder still belongs to the host. DOM focus/events
in hidden documents do not set this preference. An explicit address action keeps
address intent; submitting that address requests document intent. Newly created
blank browsers begin with address intent, while a loaded URL begins with document
intent. Find-field intent is not represented; the existing modal find UI and full
reference focus parity remain separate.

The shared layout builder no longer focuses the address field. Selection completion
restores the selected browser through this owner after attachment, only in the
actual key window. A deferred request survives background materialization until
that window becomes key. Layout-only changes restore content only when detach
actually released a content responder, so a no-focus tab insertion cannot steal
focus from sidebar/non-content controls. Address selection ranges clamp to the
current field text. No new DOM mutation/automation behavior or startup/menu-tree
ownership change is included in this repair.

The native journey now explicitly focuses its already loaded document before the
Command-9/Command-1 roundtrip, retaining the strong document-focus assertion. It
also drives the real Command-L menu route, selects a nontrivial address range,
checks that range after switching away/back, consumes an unavailable workspace
number while the address field is focused, and verifies that a genuinely new blank
tab initially focuses its address editor. These assertions are authored regression
checks. No native red/green execution has occurred locally, and source reasoning
is not substituted for that required hosted result. Portable numbered mapping/PTY
checks remain unchanged and passed; exact-source strict Swift and native execution
remain pending.

## Workspace group domain continuation (2026-10-16)

Baseline `ab7930750b2b56f8f17e14f070e7166fffa2464c`; isolated tracking worktree
`feat/workspace-groups`. Parent owns main and concurrent native integration. This
is original MIT Zmux code. Behavioral research reads only the frozen public
`docs/workspace-groups.md` and group command entry in `docs/cli-contract.md` at
cmux `3ba653b9e5afb11d0bc578031142950d6136697e`; no reference implementation,
tests or assets are imported or translated.

First useful slice: groups of explicitly provided existing workspaces, with
stable group identity, window ownership, name, collapse state and optional
window-scoped external identity. Their first member is the anchor. Group members
occupy one contiguous block of canonical window order. Creating a group compacts
its explicitly ordered members at the first requested workspace's position;
no process, pane, workspace or session is created, replaced or focused. Repeating
an external identity returns the existing group without allocating another ID.
Names are labels, not identities.

Shared actions cover create, rename, collapse/expand, add/remove member,
set-anchor, move one entire group block, and non-destructive ungroup/delete.
Removing/moving/closing an anchor promotes the next surviving member; removing
the last member deletes the group. Cross-window workspace moves keep their
existing focus/session semantics and detach only that workspace from its source
group. Closing a window removes its groups. Existing workspace reorder may not
split a group or move its anchor behind a member; rejection must leave state,
revision and allocator unchanged. Reopened closed workspaces remain ungrouped;
this slice does not invent group undo history.

Group data is additive to the current model v6 projection (existing native
presenters ignore the new group rows). A new session document version explicitly
stores group data, migrates earlier versions to no groups, and validates bounded
names/identities, live window/membership, contiguous ordering, nonempty groups,
unique membership, group IDs and global cross-kind ID collisions before restore.
Logical group/workspace/window IDs survive save/restart; ordinary shell Sessions
remain fresh under the existing durable allocation rules.

Delivery sequence: (1) demonstrate a real public core action regression, then
implement domain lifecycle/order and persistence migration; (2) expose shared
socket actions and CLI with stable selectors, stale-revision refusal and bounded
input, then verify actual socket/PTY and filesystem save/restart journeys. Run
full `scripts/check` before cohesive commits. The domain test protects ownership,
atomicity and migration; runtime/CLI tests cover distinct transport, caller scope,
real process identity and physical persistence risks. No test-only production
seam is needed.

Empty generated anchors, pin tiers, colors/icons, new-workspace placement,
explicit destructive group close, grouped undo-close, native rendering,
group-aware navigation, physical input/accessibility and full T03/V01 parity
remain open. Unsupported forms must refuse explicitly, never report scaffold
success. No native acceptance is implied by portable domain/CLI validation.

The first public core regression compiled and failed for the intended missing
`workspace_group` action, before implementation. Focused command:
`cargo test --locked -p zmux-workspace-core --test workspace_groups -- --nocapture`;
log `/tmp/zmux-t03-groups-domain-red.log`. The test checks existing-object
preservation, contiguous canonical order, idempotent allocation, atomic refusal
of a split group, anchor promotion and non-destructive ungrouping.

### Group domain/persistence cohort verified

The core owner now has bounded nonempty group records and shared lifecycle/order
mutations. All normal actions reconcile closed/moved members before commit;
ordinary single/bulk order planning also refuses a split group or changed live
anchor during preview. A focused provisional-reopen case exposed the separate
failed-launch rollback path, which now shares the same membership reconciliation.
Its intended red and final green are retained at
`/tmp/zmux-t03-groups-restore-rollback-red.log` and
`/tmp/zmux-t03-groups-domain-green.log`. An initial fixture used the intentionally
nil standalone-core namespace while asserting session export; it was corrected
to inject the same nonzero namespace required by the production persistence owner.

Session version 9 requires explicit group rows (including an empty array). The
version 1–8 readers require this new field to be absent and migrate to no groups.
Validation rejects empty, noncontiguous, duplicated, cross-window, colliding-ID
and duplicate external-identity group data. Existing legacy test constructors now
omit the future field when deriving older-version fixtures, keeping their actual
older wire contracts rather than weakening validation. New native rendering has
not been added; the existing model version 6 receives additive group metadata.

The real filesystem/PTY test groups and collapses existing workspaces, proves
all four Session IDs/PIDs and focus remain unchanged, saves exact group bytes,
shuts down and reopens the actual persistent Desktop. Logical group/window/UUID
namespace identity survives; runtime Session IDs are fresh. Focused evidence:
`/tmp/zmux-t03-groups-persistence-green.log`.

Full `scripts/check` passed 400 tests in 74 groups, one existing ignored subprocess
entrypoint, strict Clippy, notices and both C ABI gates. Final log:
`/tmp/zmux-t03-groups-domain-check-final.log`. The prior full run found a legacy
fixture still carrying the new field, then the next run passed tests but requested
an equivalent collapsed conditional in Clippy; both were repaired. Public CLI,
shared group selector/request delivery and native presentation remain pending.

The distinct public CLI/socket regression now compiles and fails at the real
`workspace group create` command because no workspace namespace dispatcher exists.
Command: `cargo test --locked -p zmux --test windows workspace_group_cli_uses_real_owner -- --nocapture`;
log `/tmp/zmux-t03-groups-cli-red.log`. It uses the actual socket, bounded native
window acknowledgement owner and running PTY children, not a mock group model.
It will verify canonical/hyphenated spellings, stable UUIDs, exact external-key
retries, hard window/caller scope, stale revision refusal, read-only listing and
non-destructive delete. No AppKit execution is claimed by its trusted boundary
acknowledgements. Source discovery will advertise only implemented forms and
keep unsupported destructive/generated-anchor/native behavior explicit.

### Group CLI/socket continuation

The shared `workspace_group` socket request resolves selectors and optional model
revision against one immutable executor snapshot, then uses the same group action
owner as native/core callers. The CLI exposes `workspace group` and the permanent
`workspace-group` spelling alias. Supported verbs are list, create, rename,
collapse, expand, add, remove, set-anchor, move, ungroup and non-destructive delete.
Create requires `--name` and 1–32 explicit `--from` existing workspace handles.
No selection is guessed and no shell is created. Group names are not identities.

Groups have a stable `workspace_group:N` reference and UUID derived from the
existing model namespace and group kind. `index:N` is a one-based group position
within canonical window order. `--window` is a hard constraint. List/create use
explicit or surviving caller window context; an invalid window-only claim cannot
fall back to the focused window. Explicit stable group/workspace targets derive
their actual owner and ignore unrelated ambient claims. All first-member and
additional-member resolution is constrained to that same window.

`--external-id` and `--idempotency-key` are equivalent, mutually exclusive create
options. Repeating an existing key in its window returns the existing group
without mutating revision or resolving now-stale initial member handles. Another
window may use the same key independently. All mutation results report actual
change, current canonical groups/order and model revision; list is read-only.
Delete/ungroup report dissolved containers and zero closed workspaces. The
`--close-workspaces` and generated-anchor cleanup forms refuse before connecting.
Group move uses a signed zero-based workspace insertion slot after removing the
whole block; values clamp and cannot split another group. Native group rendering,
collapsed navigation, generated anchors and all earlier deferred forms stay open.

The real CLI/socket/PTY regression now passes every implemented verb, canonical
and alias spelling, stable UUID/index selectors, stale-member external-key retry,
window-key isolation, wrong-window/member and stale-revision refusal, stale
ambient context refusal, read-only list/write rejection, and non-destructive
delete/ungroup. Session IDs/PIDs and focus remain unchanged through grouping and
metadata/order mutations. Evidence `/tmp/zmux-t03-groups-cli-green.log`; intended
red `/tmp/zmux-t03-groups-cli-red.log`. This test uses only the existing native
window acknowledgement boundary and makes no AppKit execution claim.

Machine command discovery uses live parser probes for all supported canonical
verbs, with truthful required arguments and remaining limitations. The actual
socket test supplies the behavioral proof; those probes alone do not establish
parity. Strict all-target Clippy passes
(`/tmp/zmux-t03-groups-cli-clippy-r2.log`). Full `scripts/check` passes
401 tests in 74 groups, one existing ignored subprocess entrypoint, strict
Clippy, notices and both C ABI gates. Final log:
`/tmp/zmux-t03-groups-cli-check-r2.log`. The first full run exposed discovery's
intentional invalid local-tmux environment leaking into list/create parser
probes. Each group discovery probe now supplies an explicit window, keeping
offline help/catalog generation independent of ambient caller claims without
changing live command routing. The existing offline discovery regression passes
(`/tmp/zmux-t03-groups-discovery-green.log`).

Domain/persistence and CLI/socket are validated portable slices. Native group
presentation/navigation, generated anchors, destructive close, pin/color/icon,
group focus/new-workspace, grouped undo and full T03/V01 parity remain pending.
This worktree does not change native sources and no AppKit or physical-input
acceptance is claimed.


Final handoff found the earlier focused `/tmp` logs no longer present. The
historical paths above describe their original executions; the retained final
full-check log contains the passing domain, filesystem/PTY and expanded CLI cases.
Both committed initial regressions were replayed from isolated `git archive`
exports, without modifying this worktree: `a1e829e` fails for the missing
`workspace_group` action and `9be231c` fails for the missing `workspace` CLI route.
Fresh evidence is `/tmp/zmux-t03-groups-domain-red-replay.log` and
`/tmp/zmux-t03-groups-cli-red-replay.log`; both exit 101 for those intended causes.
`/tmp/zmux-t03-groups-red-replay-sources.json` records exact commits and commands.
