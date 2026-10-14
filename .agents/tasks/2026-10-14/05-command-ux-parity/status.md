# Status

- **State:** in-progress
- **Updated:** 2026-10-14

## Notes
- Run 38646603782 compiled but the new visual assertion reproduced a 104/1026-pixel browser
  pane. Native content constraints still overrode NSSplitView positions. Replaced this competing
  geometry ownership with a bounded frame-based host and accessible draggable divider; retain
  split ratios and the same native geometry/PTY resize checks. Final artifact still pending.
- Geometry build 38646079812 exposed the SDK's callback-only takeSnapshot API. Adapted its
  completion to a continuation returning Sendable PNG data; require actual page pixels rather
  than silently omitting browser visual evidence. Geometry validation remains enabled.
- Run 38645312568 passed packaged native tests and produced artifact 11932730656; its downloaded
  archive, checksums, source metadata and both architectures verified. Screenshot review caught
  a cramped browser split after tab switches. Resolve divider geometry after layout, retain
  existing split ratios, and require a balanced-browser assertion before final delivery.
- Corrected-event run 38644706173 exposed an unhandled shifted menu equivalent. Reassessed
  against AppKit's documented uppercase/Shift equivalence; canonicalized Reopen to uppercase T
  with Command and refresh dynamic menu availability before synthetic event dispatch. The
  native reopen/history-consumption assertions remain mandatory; another native run is pending.
- Diagnostic run 38644079733 identifies ⇧⌘T smoke synthesis: charactersIgnoringModifiers
  incorrectly discarded Shift, invoking New Terminal. Corrected shifted characters and hardware
  keycodes; native shortcut regression remains enabled and must pass before artifact delivery.
- Hosted run 38643467867 passed universal Swift/WebKit compilation/signing and portable checks,
  but a native smoke predicate timed out. Added call-site/state diagnostics; artifact delivery
  remains pending until the failed native behavior is identified and repaired.
- P3a first slice implemented: typed browser surfaces without PTYs; public WebKit tab host,
  navigation/address/find/zoom, retained hidden-tab state and CLI browser open/navigate.
  Model migrated to v3; native smoke includes a real document/form and hidden-state retention.
  Linux checks pass; hosted build/native validation pending. No full browser parity claim.
- P2 first slice implemented: collapsible sidebar, Workspaces/Notifications tabs, persistent
  visibility, exact-target attention inbox/badges, bell/OSC parsing and explicit CLI notification
  commands. Linux checks pass; hosted/native gates pending. See references/p2-attention-sidebar.md.
- P1 first slice implemented: terminal surfaces/tabs, native controls/shortcuts/history, move/reorder,
  grouped closures and retryable fresh-shell restoration, plus actual CLI/socket commands.
  `scripts/check` passes; macOS hosted/native and interactive gates remain pending.
  See references/p1-terminal-tabs.md. P0/P1 as whole phases remain incomplete.
- Implementation started: first cohesive slice owns terminal surfaces/tabs, close history, native
  tab controls and local-control routing. P0 native measurement and exhaustive discovery remain open;
  the existing eight-terminal limit will stay in place until native measurements justify changing it.
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
Validate the P1 native artifact, then continue remaining P1 navigation/window contracts and P2 attention/sidebar; retain P0 discovery and measured resource gates.
