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
