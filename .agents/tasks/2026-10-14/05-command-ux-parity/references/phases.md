# Implementation phases and delivery gates

The [audit-driven execution plan](gap-closure-plan.md) and [work packages](closure-work-packages.md)
now define delivery order and commit slices. This file retains P0–P8 scope; it does not
require sequential completion of whole phases before managed SSH/transfer work.

This is a full parity program, not a help-text expansion. Every phase ends in a cohesive
validated commit on main and updated receipts; user-facing phases publish a macOS Actions
artifact. Keep implementation changes out of this planning task until implementation begins.
Existing roadmap M0–M7 and PR #1 requirements remain linked dependencies, not replaced goals.

The [2026-10-15 whole-product audit](full-parity-audit.md) now supplies the source-family
comparison and reconciles the initial ledgers. Per-leaf executable contracts, implementation
and runtime acceptance below remain open. Area-ledger P0/P1/P2 priority values mean urgency,
not completion of these delivery phases.

## P0 — Freeze the contract and measure the baseline

Owner: CLI/protocol and acceptance workstream. Depends on no new runtime work.

- Audit every menu/context menu, shortcut (including unbound), command palette, titlebar/sidebar,
  drag/drop and terminal/browser interaction using `ux-parity.md` and `ui-action-inventory.json`.
  Reconcile these with CLI contracts; no CLI equivalent does not remove a feature from scope.
- Expand `command-inventory.json` against the pinned source dispatcher and nested help,
  including simulator/ios, dynamic workspace/tab action values, hooks/providers, vm/cloud,
  generic rpc methods, positional defaults and aliases. Source help already contains
  `session save`, `session open` and `vm restart` beyond the supplied transcript.
- Each leaf receives accepted options/defaults, target kinds, state transitions, output
  fields, exit/error behavior, focus policy, dependencies, original test scenarios and
  implementation owner. Count aliases separately from behaviors; resolve duplicates such
  as the two diff listings. Never mark an entire umbrella implemented from one subcommand.
- Create a versioned command registry shared by parsing, dispatch metadata, offline help,
  task help, guide/skill output, completion and machine-readable capabilities. Scope
  production help to implemented commands; keep all pending commands visible in this ledger.
- Specify UUID, typed refs, indexes, `--id-format`, explicit-target precedence and caller
  defaults (`ZMUX_WORKSPACE_ID`, `ZMUX_SURFACE_ID`, `ZMUX_TAB_ID`, socket override). Keep
  existing numeric-ID/positional CLI compatibility during migration. Stale IDs must fail;
  never silently retarget an index after a concurrent topology change.
- Define socket/password settings precedence, redaction and migration without weakening
  same-user authorization. Do not inherit cmux passwords, settings or identifiers.
- Capture idle/1/8-session CPU, RSS, keystroke latency, main-loop delay, output fairness,
  close/reopen cleanup and browser-process costs on a named Mac. Set numerical budgets
  from those measurements before extending concurrency limits; Linux is not native evidence.

Exit: no unexplained catalog omissions; every known leaf/option has a contract and phase;
registry/help drift check specified; measured baseline and approved numerical budgets recorded.
A command with an unavailable dependency remains incomplete, even if its error is useful.

- Cross-cutting managed policy (see `audit-cross-cutting.md`) must share capability
  ownership, config/env precedence and live client revocation; include Computer Use and
  project panes in the later P5/P6/P8 implementation scope.

## P1 — Window, pane and surface model; real terminal tabs

Owner: Rust workspace/runtime plus thin native topology presentation. Depends on P0.

- Evolve from one session per pane to Window → Workspace → split-tree Pane → ordered
  Surfaces. A surface has stable identity, kind, selected state and its own lifecycle;
  terminal surfaces own session IDs; browser surfaces must never allocate a PTY.
- Implement shared create/select/close/move/reorder/split actions, window ownership, focus
  history, tab/workspace metadata, placement flags and dry-run semantics. Model version
  and C ABI snapshot migration precede UI consumers; replace deep-copying inactive output
  with bounded metadata projections where necessary.
- ⌘T and a plus icon create a terminal tab in the focused pane. ⌘N creates a workspace;
  splits remain ⌘D/⇧⌘D. Expose selected tab/title/close controls and keyboard traversal.
  ⌘W closes the selected surface, collapsing its pane/workspace only when empty.
- Implement ⇧⌘T, Recently Closed, workspace/window undo-close and focus back/forward/last
  in this phase, including bounded restore descriptors, failed-restore retention, missing
  container fallbacks and batch-close confirmation. Follow `ux-parity.md`; P4 persistence
  does not defer in-process terminal tab reopening.
- Keep hidden sessions alive. Switching tabs must not respawn shells or reset browser
  navigation. Closing a surface cancels only its owned resources; late events cannot revive it.
- Implement navigation family and identity/tree/list/current commands against these shared
  actions. Surface types that require later phases stay incomplete in the matrix.

Exit: ⌘T/menu/CLI produce identical topology; sibling session survives close; move/split
preserves IDs and session; multiple windows and no-focus operations route correctly;
keyboard focus, drag/reorder, IME, selected-tab behavior and actual close/reopen history
pass native checks. Killed local processes are not represented as resurrected sessions.

## P2 — Sidebar collapse and attention inbox (core product priority)

Owner: Rust attention/event state plus native chrome. Depends on P1 identities.

- Add an always-reachable sidebar toggle icon and ⌘B; preserve width and per-window state,
  resize terminal grids without respawn, and restore focus. Collapsed mode retains a
  visible notification badge/control. Cloud status must not replace notification access.
- Add a dedicated Notifications tab/inbox with unread count, per-workspace/surface badges,
  source, timestamp and preview. Clicking an entry selects its exact window/workspace/tab;
  stale destinations get a visible unavailable state, never another terminal's content.
- Separate activity (background output) from attention (bell, supported OSC notification,
  explicit notify, agent-hook completion/input-needed, configured command completion).
  Output alone must not flood the inbox. Shell integration events require documented
  event identity; do not infer agent completion from arbitrary terminal text.
- Define bounded deduplication, unread/read/dismiss/clear, navigation-to-unread, focused
  surface suppression and app-inactive delivery. Acknowledge the exact visible notification
  generation; late arrivals stay unread. Moving tabs preserves unread ownership.
- Wire notify/list/read/open/dismiss/clear, progress/status/log commands through the same
  state. Native desktop notification support is required; delivery is user-controlled and
  denied OS permission must leave the
  in-app inbox fully functional. Sanitize content; notifications never execute commands.

Exit: hidden tab attention lights workspace and inbox; foreground/idle/background cases
match the policy; click focuses the correct surface; read/clear cannot erase a concurrent
new event; collapsed sidebar and denied desktop permission retain attention access.

## P3 — Embedded browser and full browser command family

Owner: AppKit WebKit host with bounded Rust command/effect interface. Depends on P1.

- Use public `WKWebView` APIs (system WebKit), not a SwiftUI rewrite or a second embedded
  Chromium runtime. Create browser tab and browser split with URL bar, loading/errors,
  back/forward/reload/stop, find, zoom, open links and explicit download handling.
- Retain one host per live browser surface; remove subscriptions on closure; recover from
  WebContent termination without disturbing terminals. Lazy-create views and bound work
  in hidden tabs; keep PTY parsing and network waits off the main thread.
- Add isolated persistent profiles/private storage, cookie/storage/state management,
  popups, dialogs, file chooser, permissions, frame selection and navigation generations.
- Implement every browser ledger row, including selectors/accessibility snapshots, waits,
  input/interaction, evaluation/repl, screenshots, imports, tabs, console/errors, downloads,
  injected scripts/styles, design/focus mode and react-grab behavior through original code.
- Browser UI basics are P3a; automation/advanced features P3b. P3a is not browser parity.
  Investigate public API feasibility for Inspector console toggling and advanced modes;
  match deliberate unsupported outcomes in the pinned baseline; new zmux-only
  platform constraints remain unresolved parity, not silent external-browser
  fallbacks or falsely successful no-ops. No private WebKit KVC copied from cmux.
- Untrusted pages cannot access the local command socket or native terminal actions.
  Separate page JS from privileged local automation; validate bridge origin/frame/surface,
  file access and external URL schemes. Preserve TLS failures. Any explicit trust exception needs consent, scoped policy and
  revocation; never silently bypass certificate checks.

Exit: native local-fixture browsing/navigation/forms/downloads/profile isolation succeeds;
all browser leaves have behavioral evidence including cross-origin frame limitations,
close-during-eval/navigation cancellation, process crash and bounded slow-page behavior.

## P4 — Startup, persistence and customization

Owner: storage/settings/launch integration. Depends on P1, browser restore on P3.

- Path invocation launches zmux if needed and opens the intended cwd without shell-string
  interpolation. Implement multi-path/url open, creation cwd/command/layout/placement flags.
- Versioned atomic persistence, safe restore/export/import, checkpoints/fork and local
  tmux/zellij lifecycle. Restart does not imply resurrection of a dead local shell process;
  distinguish restored layout from preserved remote/multiplexer sessions.
- Implement settings/config/schema/docs/guide, themes, shortcuts, reload, terminal import,
  sidebar templates/tools, feedback and welcome/feed. Back up user config before migration;
  invalid reload preserves the last working config. Use zmux-owned config paths.
- Ghostty-specific compatibility requires a permissive engine/config adapter feasibility
  decision. Current vt100 renderer cannot honestly claim Ghostty feature equivalence.

Exit: restart/restore/corrupt file recovery and import dry-run pass; no secrets in exports;
settings and shortcuts take effect predictably without a restart where promised.

## P5 — Automation, inspection and content tools

Owner: local command/event transport plus content surfaces. Depends on P1–P4 as relevant.

- Implement bounded event subscriptions with cursors, replay gaps, reconnect, cancellation
  and slow-consumer eviction; automation rules, hooks, todo, signals and command execution.
- Complete send/key/paste/read/selection/scrollback, identity/tree/process/resource/health
  inspection, capture and recording, markdown/diff, comments/reviews and history/vault.
- Implement tmux-compatible pane/buffer/history/control operations through shared owners,
  not a second model. Frame geometry, bracketed paste and exact bytes are behavioral contracts.
- Platform capture permissions must be exercised, not replaced by success-shaped mock files.

Exit: fixtures establish observable side effects and result schemas for every assigned
row; event floods and disconnected subscribers do not stall UI or other sessions.

## P6 — Agent workflows and integrations

Owner: adapters atop events/attention/persistence. Depends on P2/P4/P5.

- Implement agent messaging/inbox/replies, hibernation/wake, provider hooks and feeds,
  team launcher wrappers, account handling and checkpoint/recovery workflows.
- External CLIs are explicit dependencies: version discovery, argument-safe invocation,
  actionable missing-tool errors and user-controlled installation. Never report a wrapper
  as working when its upstream tool is absent; test both available and unavailable paths.
- Port product-specific names such as coderouter/glaeda only as compatibility entrypoints
  to independently implemented behavior or legitimate documented integrations. Provider
  APIs/credentials remain explicit dependencies; no copying licensed service internals.

Exit: agent completion/input-needed reaches correct notification target, duplicate events
are coalesced and actual provider fixtures validate each supported integration.

## P7 — Remote terminal and cloud command parity

Owner: remote runtime and independent control plane. SSH depends on shared identity/actions,
not cloud hosting. Implement managed SSH with file/image drop and paste as a priority journey;
see [traced contract](ssh-transfer-contract.md). Only hosted VM/cloud journeys depend on the
deferred provider/database decision.

- SSH first, together with remote file/image drop and clipboard upload, transfer progress,
  cancellation, authenticated route reuse and origin-bound remote-path insertion.
  Include host-key validation, cwd/command transport, daemon negotiation, reconnect,
  output replay, resize, detach/attach/cleanup and process ownership proven on a second host.
  Then mosh/tmux/session move and all remote discovery/status commands.
- Complete auth/team/account commands and every vm/cloud verb (including nested verbs),
  networking, ports, files, snapshots/forks, layouts, agent/desktop/tui and device presence.
  This requires VM/relay infrastructure; task 04 identity/device registration is not enough.
- Use https://zmux.w3dev.app and deploy only through GitHub Actions. Hosting, database
  operations, budget, secret provisioning and provider selection remain deferred discussion.
  Link all existing PR #1 parity rows; do not lose MFA/mail/organization requirements.
- Add record-retention cleanup, device heartbeats, revocation and isolation; never execute
  a failed remote request locally or grant remote commands unscoped local effects.

Exit: authenticated two-host and deployed-service journeys, ownership failures, reconnect
soak and explicit numeric responsiveness/resource budgets pass. Host-blocked rows remain open.

## P8 — Platform-specific and advanced closure

Owner: platform adapters and final parity audit. Depends on previous relevant phases.

- Complete simulator/iOS command families and their required client/companion behavior;
  account for OS availability and device prerequisites without removing them from scope.
- Complete sudo/Touch ID, diagnostics, raw RPC policy, capture/recording gaps, bindings,
  popup/copy mode and all remaining advanced commands in the ledger.
- Validate independent transport diagnostics behind compatibility names (e.g. iroh-diag);
  different implementation internals must be documented, not presented as upstream service use.
- Expand and resolve all wildcard inventories, reconcile help/dispatcher/options to the
  frozen baseline, and assign later source changes to a new baseline rather than moving
  this task's completion target without review.

Exit: every command leaf/alias/option is verified or has an explicitly user-approved
scope exception. Dependency-blocked is not complete. Native UX, accessibility, privacy,
restart and remote-stress gates pass; publish final Actions artifact and evidence matrix.
