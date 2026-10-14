# Status

- **State:** planned
- **Updated:** 2026-10-14

## Notes
- User clarified that ⌘T is an example: full interaction parity is required, including reopening
  closed terminal tabs. Added separate UI inventory, broad UX gap matrix and explicit P1
  ⇧⌘T/Recently Closed, workspace/window recovery, close-other and focus-history contracts.
- Static source confirms general closed-item recovery despite a legacy browser-named action.
  Menu/context/gesture audit is still open; CLI coverage is not a product-parity completion gate.
- User requested a new agent-task covering all CLI commands, ⌘T terminals, embedded browser,
  sidebar collapse and notification/attention behavior. This change is planning only.
- Read-only source review confirms WebKit browser hosting, per-pane surfaces, ⌘T, sidebar
  toggle and centralized unread/notification behavior. No source/assets copied into zmux.
- Initial inventory has 428 command entries, including source additions beyond supplied help.
  This is not an exhaustive option/dispatcher audit; P0 must expand wildcard families and flags.
- Priority: shared surfaces/tabs, then sidebar and attention inbox, then browser UI/automation.
- Current help's small command set remains accurately described as partial; no functionality
  was added by this plan. Every pending command retains an implementation phase and evidence gate.
- Production origin remains https://zmux.w3dev.app; host/provider selection is deferred and
  deployments must use GitHub Actions. Cloud/VM parity remains open until actually delivered.

## Next action
Execute P0 contract expansion and native baseline measurement, then P1 shared surface model.
