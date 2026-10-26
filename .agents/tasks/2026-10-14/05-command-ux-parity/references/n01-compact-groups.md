# Compact chrome and native workspace groups

User screenshots (2026-10-16) show two excess sidebar control rows and no visible
group controls. Deliver one titlebar-height icon strip beside the traffic lights,
a sidebar toggle that remains reachable when collapsed, and collapsible workspace
group headers. Keep notifications accessible from a badged bell that returns to
workspaces on a second click. Secondary actions live in the add/options menu.

Use original AppKit UI and the shared Rust workspace_group actions. Groups contain
existing workspaces, are window-scoped, preserve live terminals and focus, persist,
and dissolve without closing sessions. Expose create/rename/ungroup and move/remove
workspace through native context menus. Collapse does not terminate terminals.

Validation: existing Rust domain/CLI tests plus a native journey through real icon
buttons and group menus. Check geometry at minimum/default widths, sidebar hide/show,
notification return, collapsed children, membership changes, unchanged child PIDs,
selected content and first responder. Retain a screenshot and GitHub build artifact.
Accessibility Save-dialog test remains explicitly deferred by user.

## Native validation

- Both Swift slices and universal packaging pass. Initial sidebar run38882761611
  failed before app construction because AppStartup omitted the new fixture flag;
  e960f88 fixes that route.
- Actual runs38884369718/38885891507 create groups and change membership, then miss
  the header pointer gesture. Hit-test consolidation alone did not repair it.
- Independent AppKit probe38887702536 proves a cell-less NSControl retains its
  target/action and forwards a label pointer event. No NSActionCell repair needed.
- Run38888028869 records valid header bounds/visible rect/hit/target/action but
  key=false after a sheet. No mouseDown reaches the header; first click activates
  the inactive window. The fixture now awaits active/key window after sheet end,
  matching the existing window lifecycle fixture, and asserts key-window identity
  throughout mutations. Run38889028230 confirms direct binary launch cannot satisfy
  initial foreground activation. The runner now uses foreground Launch Services,
  matching the existing window lifecycle fixture, with a nonce/SHA-bound app PID
  and a separate post-shutdown physical cleanup receipt.
- Focused workflow now packages Apple Silicon on macos-15; both Swift source slices
  remain checked. Regular full workflow retains universal packaging. All nine
  interaction flags and physical app/socket cleanup assertions remain required.

## Accepted native slice

Run [38890159245](https://github.com/ashutoshpw/zmux/actions/runs/38890159245)
at `cdbb392570e0b8568ad3e2329f5021e3e15fc37e` passes all nine native flags.
The actual header pointer log records key=true, matching namespace and a dispatched
mouseDown. Create/membership/rename/remove/ungroup use real menus and name sheets;
collapse preserves selected terminal/PID/responder. A real second-window group is
excluded from the source sidebar and group menus. Both toggle states and bell
navigation pass, followed by physical shutdown and exact app PID absence.

Parent verified both PNGs, ZIP integrity/checksum, embedded source SHA/lowercase app
name and equality of the archived executable with the native process receipt.
App PID7589; Launch Services driver7586; owned socket absent; cleanup complete.
Archive SHA256 `a507e38013c385a519118e97af2fd595159a608245852e360b50aeb604166895`.
[Apple Silicon preview artifact](https://github.com/ashutoshpw/zmux/actions/runs/38890159245/artifacts/12042350763).
Broader product parity remains in progress; this is acceptance of the compact
sidebar/grouping slice. Save dialog Accessibility remains explicitly deferred.

The subsequent workspace-menu screenshots expose missing actions beyond this
slice, including empty groups, pinning, metadata, attention controls and batch
closing. See [N02 menu correction](n02-workspace-context-menu.md). The nine accepted
flags above do not establish context-menu completeness or full grouping parity.
