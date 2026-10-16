# Workspace context menu parity correction

**Audited:** 2026-10-16. **State:** partial; named attention/UUID/ordering actions
implemented, broader menu behavior remains incomplete.
Reference: read-only cmux `3ba653b9e5afb11d0bc578031142950d6136697e`.
Audit baseline: zmux `8258ccb` (application source unchanged from `cdbb392`).

## Delivery failure

The user's two workspace-menu screenshots expose a known gap, not an equivalent
menu with different styling. The original terminal audit already listed missing
pinning, metadata, unread, bulk closing, IDs/links, Finder and remote context actions.
The compact-sidebar acceptance exercised toolbar geometry, basic existing-workspace
groups, focus and child lifetime. It did not establish workspace-menu completeness.
Sharing that preview did not satisfy the task's full-parity artifact criterion.

The correction must close behavior and its native entrypoints together. Adding
labels, passing an implemented-subset fixture or updating CLI help cannot close it.
No product change is delivered by this audit record.

## Screenshot inventory and current implementation

This table records the audit baseline `8258ccb`. Subsequent implementation is
tracked below; it must not be mistaken for completed native acceptance.

| ID | Reference action | Current zmux behavior / required work |
| --- | --- | --- |
| CM01 | Pin / Unpin Workspace | Missing model, ordering tier, protected-close policy, persistence and native action. |
| CM02 | Mute / Unmute Workspace Notifications | Missing per-workspace policy and menu. Global desktop-notification permission is a different behavior. |
| CM03 | New Empty Workspace Group | Missing. Current group invariant requires an existing workspace anchor; the options-menu group action also groups an existing workspace. |
| CM04 | New Group from Workspace | Implemented for an existing workspace, with a name sheet. Generated anchors and full grouping semantics remain open. |
| CM05 | Move to Group | Implemented conditionally when another group exists in this window. Match reference enabled/disabled presentation and stale-menu handling. |
| CM06 | Remove from Group | Implemented conditionally for grouped workspaces. Match reference state presentation. |
| CM07 | Rename Workspace | Implemented. Manual-title override/reset versus terminal-derived naming is not established. |
| CM08 | Edit / Clear Workspace Description | Missing workspace description field, durable mutations, native editor and projection. Status/log metadata is a different feature. |
| CM09 | Workspace Color | Missing manual workspace color and clear/custom/palette UI, persistence and sidebar rendering. Existing status-entry colors do not implement this. |
| CM10 | Move Up / Down / to Top | Shared ordering exists, but controls are nested in Reorder Workspace. Expose the reference interaction and verify group/pin constraints. zmux also has Move to Bottom. |
| CM11 | Move Workspace to Window | Existing-window submenu appears only when another window exists. New Window destination is missing; shared native capacity is currently two windows. |
| CM12 | Close Workspace | Implemented with confirmation and shared close. Pin/remote-specific protection and complete reference history behavior remain open. |
| CM13 | Close Other Workspaces | Missing workspace batch action and native control. Close Other Surfaces does not implement it. |
| CM14 | Close Workspaces Below | Missing workspace range selection, batch action and native control. |
| CM15 | Close Workspaces Above | Missing workspace range selection, batch action and native control. |
| CM16 | Mark Workspace as Read | Shared workspace-scoped notification MarkRead exists; no workspace context control. Full workspace-indicator clearing requires the CM17 model too. |
| CM17 | Mark Workspace as Unread | Missing manual workspace unread state independent of notification records. Reopening an old notification or inserting a fake notification is not equivalent. |
| CM18 | Clear Latest Notification | Exact notification dismissal exists; workspace menu lacks latest-item selection and wiring. Must remove only the captured item and preserve arrivals after menu opening. |
| CM19 | Notifications submenu | Existing bounded notification records and exact-target opening can be reused; no per-workspace menu, unread markers or empty state. |
| CM20 | Copy Workspace ID | Stable UUID identities exist; workspace clipboard action is missing. Copy the durable UUID, not the display index. |
| CM21 | Copy Workspace Link | Missing native workspace navigation link contract, registration/dispatch and clipboard control. Printing a URL alone is insufficient. |
| CM22 | Show in Finder | Native action is missing. CWD data exists for terminal/surface owners; resolve the right workspace, validate locally off the UI thread and do not interpret a remote path as a local folder. |

Some cmux screenshot rows are disabled rather than omitted. zmux currently omits
several conditional actions. Equal menu length alone is not an acceptance criterion:
valid target resolution, disabled reasons, shortcuts, checkmarks, separators and
effects must agree with the relevant state.

## Source evidence

Reference sources inspected at the frozen commit:

- `Sources/TabItemView+WorkspaceContextMenu.swift`: action list, multiple targets,
  conditional metadata/remote rows, color palette, close ranges and New Window.
- `Sources/TabItemView+WorkspaceGroups.swift`: group action surface.
- `Sources/TabItemView+WorkspaceNotificationsMenu.swift`: notification menu,
  empty state, unread marker and exact notification opening.
- `Sources/TerminalNotificationStore.swift`: manual workspace unread is distinct
  from record unread; clear-latest removes one exact stored notification.
- `Sources/ContentView.swift`: shared row-action dispatch.
- `Sources/WorkspaceFinderDirectoryResolver.swift`: directory resolution and
  asynchronous existing-directory validation before Finder presentation.

Current zmux owners:

- `apps/macos/Sources/App.swift`, `workspaceMenu(for:)`,
  `appendWorkspaceGrouping`, `workspaceGroupMenu`, `readNotification`,
  `dismissNotification`, `promptToRename`, `closeWorkspaceFromMenu`.
- `apps/macos/Sources/ApplicationCoordinator.swift` and `NativeBridge.swift`:
  native action queue and bridge.
- `crates/workspace-core/src/lib.rs`: Workspace and shared actions.
- `crates/workspace-core/src/workspace_groups.rs`: nonempty contiguous groups.
- `crates/workspace-core/src/windows.rs`: current two-window capacity.
- `crates/desktop-runtime/src/metadata.rs`: PR/todo/status/progress/log metadata;
  no manual workspace description or row color.
- `crates/desktop-runtime/src/attention.rs` and `lib.rs`: scoped mark-read,
  dismiss/clear, generation-aware notification targets and opening.
- `crates/desktop-runtime/src/workspace_order.rs`: shared revision-bound ordering.

## Additional conditional cases beyond these screenshots

The frozen reference also exposes manual-name reset, description clear, remote
reconnect/disconnect and copy-SSH-error entries, todo/checklist actions and
multi-selection variants. The current four-row screenshot cannot exercise these.
Track and verify them alongside this menu, not only after another user screenshot.
Group-header, terminal-tab, terminal-link and browser context menus remain separate
obligations in the broader audit; this table is not a whole-product completion claim.

## Cohesive implementation sequence

- [ ] **Shared workspace state:** add durable description/color/pin policy and
  explicit manual attention/mute state in their Rust owners. Define migrations,
  group/pin ordering, close protection and reopening behavior before native wiring.
  Route CLI/menu/shortcuts through the same actions.
- [ ] **Group/window and closing behavior:** implement empty-group lifecycle,
  create-window-and-move, and other/above/below close planning. Capture exact UUID
  targets in the owning window. Confirm the proposed close set; cancellation must
  preserve all children. Validate the complete set before mutation; bound cleanup
  off the UI thread. Define history/reopen order, anchor promotion and pin policy.
- [ ] **Native menu completion:** expose the full inventory, direct ordering,
  scoped attention controls, notification submenu, UUID clipboard and valid Finder
  action. Implement and verify round-trip workspace links. Include conditional
  remote/metadata and multiple-selection cases, plus matching shortcuts and states.
- [ ] **Combined acceptance and delivery:** execute the matrix below on the final
  packaged app in GitHub Actions. Commit cohesive passing slices; bind receipts
  to source SHA and packaged bytes. Leave broader V01 gates open until satisfied.

## Behavioral acceptance matrix

Every CM row needs an implementation owner, shared behavioral result and real
native-menu result. A label-presence assertion alone is insufficient.

- Open menus for ungrouped/grouped/pinned/local/remote workspaces, inactive rows,
  collapsed groups, multiple selections, one/multiple windows, no notifications,
  read/unread/manual-unread and muted states. Compare enabled states and variants.
- Invoke a context action on workspace B while workspace A is active. Preserve A's
  focus and children unless the action explicitly selects/closes/moves them.
- Reopen menus after a move/reorder/restore. Exercise close/move/new notification
  while a menu or confirmation is open. Reject replaced/stale identities rather
  than applying a captured display index to a different workspace or window.
- Description/color/pin/mute and group state survive save/restart as specified;
  legacy snapshots migrate without changing existing sessions or identities.
- Mark-read clears the intended indicators. Manual unread works without records.
  Mute/unmute follows the reference's recording/delivery semantics. Clearing latest
  preserves older records and newer arrivals. Notification opening selects the
  exact surviving workspace/surface and never borrows an unrelated foreground tab.
- Bulk closing respects pinned protection, confirmation cancellation, window/group
  boundaries, exact child cleanup and recently-closed recovery. No unintended child
  is restarted, moved, killed or hidden by a successful action.
- UUID copy is stable across ordering. Workspace links round-trip in running/cold
  app states and fail clearly for missing targets. Finder resolves valid local
  directories asynchronously and handles deleted/unknown/remote paths correctly.
- Run `scripts/check`, both Swift architecture checks and the packaged native
  context-menu journey. Inspect screenshots and retain final-SHA execution and
  physical shutdown receipts. The user-deferred Save Accessibility test remains
  outside this gate; it does not block any menu implementation above.

## Attention and identity implementation

Source now exposes direct Move Up/Down/Top/Bottom controls, scoped Mark Workspace
as Read, exact Clear Latest Notification, a bounded workspace Notifications
submenu with closed-target/empty states, and Copy Workspace ID using the durable
UUID. Workspace context menus refresh from the current projection when opened;
their namespace guards reject a replaced desktop. The notification actions share
the same acknowledged Rust owner as the status menu, retain epoch/cursor/exact
targets, and never borrow foreground workspace scope.

The packaged sidebar/status journey now invokes real context read/clear/clipboard
selectors on an inactive workspace while another window retains unread attention.
Hosted execution remains pending. Pinning, mute/manual-unread, descriptions/colors,
empty groups, batch closing, deep links, Finder and other open rows above remain
unfinished; these additions do not close the whole context-menu or V01 gate.

Subsequent native acceptance: run38912740671 at `38bfc72` passes the actual
inactive-workspace read/clear/UUID clipboard actions, preserving the selected
workspace and another real window's unread attention. See the
[N03 receipt](n03-macos-menu-bar.md#accepted-packaged-statusattention-slice) for
package/source/executable verification and the complete focused native scope.
The remaining CM rows above stay open; conditional controls are not claimed as
the complete frozen menu.
