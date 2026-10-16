# macOS menu bar parity correction

**Audited:** 2026-10-16. **State:** status/attention implementation in progress;
packaged native acceptance pending. Advanced action owners remain open.
Reference: read-only cmux `3ba653b9e5afb11d0bc578031142950d6136697e`.
Audit baseline: zmux `8258ccb` (application source unchanged from `cdbb392`).

The user's screenshot shows the application status menu in the top-right macOS
menu bar. This is separate from the Dock, the main application menus, desktop
notification banners and the in-window sidebar bell.

## Verified cause at audit baseline

`apps/macos/Sources` contains no `NSStatusBar`/`NSStatusItem` or `MenuBarExtra`
owner. `AppDelegate` starts the `ApplicationCoordinator`; window hosts install
`NSApp.mainMenu`, but neither creates a system status item. macOS therefore has
no zmux icon to show. This is an unimplemented native feature, not an icon-hiding
setting or an Accessibility/notification permission problem.

The earlier ledgers cover notification behavior, global search, resource tools,
updates and a Computer Use menu-bar projection separately. They do not provide
an explicit acceptance matrix for this complete primary status menu. The audit
must not be described as exhaustive while this surface has no delivery gate.

## Reference behavior and current gaps

| Surface / action | Current zmux / required work |
| --- | --- |
| Status icon and unread summary | Missing original icon renderer, retained native status item, unread state/tooltip and accessible identity. |
| Recent notification previews | Bounded shared records exist; status-menu projection and action routing are missing. Preserve exact notification identity, title/body, timestamp and read state. |
| Show Notifications | Sidebar presentation exists; add global menu activation through the correct window host. |
| Jump to Latest Unread | Shared exact-target navigation exists; status-menu action/state is missing. Verify ordering against the reference. |
| Mark All Read | Shared global mark-read exists; native menu control/state is missing. Include manual workspace attention when implemented. |
| Clear All | Cursor/epoch-aware shared clearing exists; menu control/state is missing. Preserve arrivals after the captured clear boundary. |
| Show main window / menu-bar-only behavior | Missing status entrypoint and preference/lifecycle behavior. Reuse the existing coordinator, never create another desktop/session owner. |
| Search All Windows | Complete global search and status-button popover anchoring remain open; an active-terminal find action is not equivalent. |
| Task Manager | Native process/resource window and action are missing; underlying inspection primitives alone do not implement the window. |
| Sleepy Mode | Native mode, settings, power/display behavior and clean dismissal remain open. |
| Keep Mac Awake | Missing explicit power-assertion owner, toggle/checkmark and cleanup. It must not silently alter the user's system settings. |
| Start Profiling | Missing bounded profiling lifecycle, output handling and status-menu action. |
| Cloud machines/account/create/more | Cloud identity/service foundations are partial; corresponding shared menu entries and complete VM behavior remain open. Hosting deferral is not a waiver for menu or service implementation. |
| Check for Updates | Verified updater/release authority and native action remain open; opening an arbitrary release URL is not equivalent. |
| Preferences | Full preferences owner and status-menu entrypoint remain open; Cloud Account alone is not the complete preferences UI. |
| Quit zmux | Shared application shutdown exists; status-menu action is missing. Route through normal application termination and await owned cleanup. |

Read-only reference sources: `Sources/App/MenuBarExtraController.swift`,
`Sources/App/MenuBarProfilingMenuItem.swift`, `Sources/TerminalNotificationStore.swift`.
They establish behavioral scope only. No cmux code, icon or other asset may be
copied or translated into the MIT implementation.

## Implementation boundaries

- [ ] Add an original AppKit controller retained by the application coordinator,
  created after successful startup and removed during shutdown. One status item
  per application, independent of the count/visibility of workspace windows.
- [ ] Render an original compact template icon for light/dark menu bars, with
  readable attention state, tooltip and accessibility label. Use the shared
  application snapshot, not whichever window happens to own the sidebar.
- [ ] Build a bounded menu projection and reuse the existing attention operations
  and exact notification-opening receipts. Menu callbacks enqueue work; no PTY,
  network, disk scan, process wait or blocking bridge request on the UI thread.
- [ ] Complete search/resource/power/profiling/preferences/update owners in their
  respective work packages, then expose their shared actions here and in their
  other entrypoints. Empty labels or inert controls cannot satisfy an action.
- [ ] Reuse the same cloud entries/state as the main Cloud UI. Asynchronous refresh
  must not destroy an active submenu or leak credentials. Local Docker/PM2 service
  and `zmux.w3dev.app` tunnel remain the user's selected deployment approach.
- [ ] Reconcile visibility preferences, menu-bar-only mode, window restoration,
  last-window closure and application termination. Release status observers,
  power assertions and owned workers through the shared shutdown path.

## Acceptance

- [ ] Packaged GitHub Actions macOS app displays the actual system status item;
  retain a screenshot of the OS menu bar and its open menu, not an in-app imitation.
- [ ] Exercise zero/read/unread/muted/closed-target notifications, multiple windows,
  a minimized/hidden window and a notification arriving while the menu is open.
  Verify exact target activation, enabled states, count/tooltip and captured clears.
- [ ] Verify startup failure, repeated window creation/closure, hide/show preference
  and normal termination leave no duplicate item, observer or owned power assertion.
- [ ] Invoke each visible action and verify its real behavior through the production
  owner. Shared state and menu presentation must agree after failures and restarts.
- [ ] Verify both Swift architectures, focused shared tests, `scripts/check` and
  a real packaged native status-menu journey. A passing sidebar suite does not
  exercise this surface. Accessibility Save-dialog testing remains deferred and
  does not block adding the status menu.

This correction supplements [N02 workspace context menu](n02-workspace-context-menu.md)
and the broader audit. Neither an icon-only slice nor this audit record establishes
full cmux parity or authorizes describing a new artifact that way.

## Status and attention implementation

`StatusMenuController` now creates one original template icon/NSStatusItem, retained
by `ApplicationCoordinator` after startup and removed before shared shutdown.
Its menu exposes recent previews, Show zmux, Show Notifications, Jump to Latest
Unread, Mark All Read, Clear All, Cloud Account, desktop-notification permission,
Keep Mac Awake and normal application Quit. Show zmux restores minimized/hidden
live windows through the existing owner without starting a second Desktop.
Keep-awake uses an owned IOKit assertion and releases it on toggle/shutdown.

The menu reads the complete application snapshot across windows. Rendering updates
only when attention/availability state changes; terminal output alone does not
rebuild NSMenu. Open menus retain their displayed targets and history boundary.
Native `mutate` actions reuse the shared Rust notification filter/mutation owner
with epoch, model namespace, cursor and optional exact workspace/record scope.
Mutations return no focus/activation receipt. Normal preview opening reuses the
existing acknowledged exact-target notification navigation.

The actual Desktop regression was committed first as `5b086a0`; it fails because
native `mutate` is absent. The same test passes after the shared repair and also
covers mixed-scope rejection and exact-record removal. Logs:
`/tmp/zmux-status-menu-owner-{red,green}.log`.

The existing packaged sidebar suite now additionally exercises the actual system
status button/menu, context attention/UUID actions on an inactive workspace,
notifications in another real window, arrivals after menu opening, exact preview
activation, minimized-window restoration and keep-awake release. It retains the
actual icon PNG and `native-status-menu.json`; all ten additional flags and
post-shutdown item/assertion removal are required. No Save-dialog/Accessibility
test is added or re-enabled. Hosted execution remains pending.

Full search, task manager, Sleepy Mode, profiling, preferences, verified updating,
cloud VM creation/machine menus and menu-bar-only preferences remain open. There
are no inert menu entries claiming those owners are implemented.

Native run `38903386218` at `96e13ea` passes both Swift slices and host packaging,
then fails on the actual preview lookup after captured mutations: the closed
status menu still held its old items. Physical app/socket cleanup and PID absence
pass; no app artifact is published. `menuDidClose` now applies coalesced snapshot
changes immediately, including when the Desktop becomes quiet after clearing.
The same fixture remains unchanged. Early Show actions also wait for a pending
window's native-ready receipt; they must not create a duplicate startup window.

Run `38906067122` at `84c13ad` passes the repaired preview lookup and exact
cross-window opening/inbox steps, then catches minimized-window restoration:
minimizing the target transfers key focus to its sibling, so choosing the current
key window restores the wrong one. The native coordinator now retains bounded
minimization history and restores the most recently minimized live host first.
AppKit delegate callbacks reconcile this presentation state; close/restore/shutdown
remove stale identities. The same minimized-window and last-window assertions
remain required. Failed-run physical app/socket cleanup and PID absence pass.

## Lifecycle reassessment after the third native run

Run `38907593402` at `f712e5e` passes the minimized-window point and reaches the
new close-all scenario. Its final close is a `terminate` request, so normal shared
shutdown removes the control socket while the CLI is still observing its receipt.
This is the retained regular-mode contract, not evidence that a background app
should have zero windows: `workspace-core/src/windows.rs` rejects deleting the final
window, `desktop-runtime/tests/windows.rs` requires preservation of the pre-close
layout/process until termination, and T03 explicitly requires save/quit on final
close. The frozen reference has a separate explicit menu-bar-only preference;
that preference remains unfinished here.

The new fixture incorrectly assumed menu-bar-only semantics. Reassessed after
three native failures, it now exercises actual `NSApp.hide`/Show recovery, requiring
the same status item and original child identities. Show explicitly unhides the
application. The receipt is named `hidden_window_recovery`; it does not claim
last-window background recovery. Existing last-close/save/quit tests remain
unchanged, and final status-item/IOKit cleanup remains required. No regular-mode
window policy is changed, no failing product regression is disabled, and no
menu-bar-only implementation is claimed. Physical cleanup/PID absence passed
on the failed run; the corrected native journey still needs hosted acceptance.

Run `38909353414` at `5d1858d` passes prior checks but fails hidden-application
restoration. Before another production activation change, isolate public AppKit
ordering/activation behavior in `tools/native-activation-probe/main.swift`, through
the manual `native-control-probe.yml` activation scenario. It records actual
hidden/active/visible/key-window state for order-then-activate, activate-then-order,
waiting for active state and exact-process NSRunningApplication activation, with
unique-PID Launch Services controls. This is diagnostic evidence, not product
acceptance. The probe has no Desktop/PTY, uses its own unique bundle ID and exits
after its finite observation sequence. Original control-action probing is retained.
