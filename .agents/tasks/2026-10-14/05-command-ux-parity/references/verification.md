# Parity ledger and verification contract

`command-inventory.json` contains 428 initially expanded command entries across the nine
help groups. It is a planning inventory, not 428 implemented commands or a complete count
of the upstream dispatcher. Wildcards, dynamic action names, options, aliases and nested
provider commands must be fully expanded in P0. Every original help family is assigned;
source-discovered additions are included. `cloud` aliases the complete vm subtree.

Baseline statuses are conservative: same-name existing CLI operations are partial because
flags, target resolution, focus and output differ. Missing means absent from the current
CLI contract; UI-only auth is not CLI auth parity. Preserve zmux-specific doctor/status,
prototype and legacy split-pane/send-text commands as compatible additions.

## Per-entry implementation record

Before implementing an entry, add these fields to its record or linked contract:

- Canonical command, aliases/default verb, baseline source pointer and original behavior spec.
- Every positional argument/flag, valid combinations, defaults and precedence.
- Window/workspace/pane/surface/tab target resolution; caller env and explicit target rules.
- Preconditions, effects, focus policy, output schema, exit codes, timeouts/cancellation.
- Implementation owner, phase/dependencies, external tools/OS/provider prerequisites.
- Positive/negative behavioral scenarios and exact regression command, then source SHA,
  local result, native run/artifact and authenticated/remote evidence when applicable.

Lifecycle: needs-contract → planned → implemented-unverified → verified. Dependency-blocked
and platform-limited are open states. Never count a printed help line, parser acceptance,
mock return or informative unavailable error as completed command behavior.

## UI parity is a separate completion dimension

The 428-entry CLI inventory is not a complete feature inventory. Reconcile the independent
UI action inventory and `ux-parity.md` against menus, context menus, command palette,
shortcuts, gestures, terminal-engine bindings and stateful recovery flows. Every exposed
entrypoint needs an enable/focus/lifecycle contract and evidence. Include ⇧⌘T, Recently
Closed, batch-close cancellation, focus history and missing-container restore in native
acceptance; validating only create/split misses the core lifecycle.

## Required acceptance layers

1. **Contract coverage:** compare registry keys/aliases/options to the frozen inventory and
   recursively inspected dispatcher. Every omission fails parity closure; additions after
   the freeze are separately recorded. Detect help entries without handlers and handlers
   without docs. Offline help/task help/guide must not require an app, socket or network.
2. **Shared owner behavior:** equivalent UI/menu/shortcut/CLI actions have identical state
   transitions and errors. Assert invariants at Rust owners, not repeated mock dispatch calls.
3. **Public CLI integration:** actual executable + live socket + actual PTYs; stale/foreign
   IDs, wrong owner, auth mismatch, invalid flags, no-focus, timeouts and uncertain mutations.
   Verify output and exit semantics, not only command-name existence.
4. **Native UX:** packaged app on macOS tests ⌘T tab creation without split, tab selection/
   close/move, keyboard focus, sidebar collapse/restore, browser page interaction, notification
   badge/inbox/deep-link/read behavior and restart. Capture normal product states as well as
   test output; inspect screenshots and do manual keyboard/IME/VoiceOver checks.
5. **Browser:** local fixture HTTP server with redirects, navigation races, slow pages, iframe
   boundaries, form controls, downloads and permissions. Real WebKit and per-profile storage;
   no fake browser response dictionaries. Test native-to-JS boundary and hostile page isolation.
6. **Attention:** multiple hidden/visible tabs and windows; explicit notify/bell/hooks; duplicate,
   late and moved-surface events; read/dismiss races, unread persistence, denied OS notifications,
   collapsed sidebar and event floods. Every notification action uses the authoritative store.
7. **Remote/cloud:** two-host execution identity, SSH host keys, disconnect/reconnect/daemon
   restart, owner isolation, backpressure and input ambiguity. Real disposable PostgreSQL and
   deployed auth/VM journeys; Actions-only deployment at the chosen host. No local fallback.
8. **Resource/stability:** use P0 numeric budgets and record machine/build/workload, p95 input
   and main-loop delay, peak/steady RSS, idle CPU, worker cleanup, WebKit process lifecycle,
   output floods and long remote-session soak. A different stack alone is not stability evidence.

## Commit and delivery cadence

Run `scripts/check` before each cohesive commit, plus changed-area native/cloud checks.
Avoid one commit per command and avoid one monolithic parity commit: group related behaviors
sharing the same owner and validation gate. Each phase updates task status and ledger evidence.
Use `main` per repo instructions, retaining all prior user changes. Native artifacts must come
from successful GitHub Actions, with source/checksum/architecture/smoke receipts inspected.

P1/P2/P3a provide early usable UX artifacts; these are explicitly intermediate deliveries.
Full parity requires all ledger rows/options verified and no unresolved wildcard families,
unless the user explicitly changes scope. Hosting deferral does not waive cloud command scope.

## Broader source audit receipt — 2026-10-15

Compared cmux `3ba653b9e5afb11d0bc578031142950d6136697e` with zmux
`69342a8f036037736c6eb9a500e479ac41e32d93`. The
[whole-product audit](full-parity-audit.md) and its linked area reports are the
current comparison; original discovery ledger status fields are retained as history.

- `scripts/check`: passed (45 Rust tests, formatting, clippy, dependency notice
  inventory and C ABI harness). The inventory still reports two notices needing
  upstream text review; no new dependencies were introduced.
- Audit JSON parses; feature identifiers are unique within each area. Reconciliation
  contains exactly the original 428 command IDs and 171 shortcut IDs, each routed to
  an existing report. Routing is not a per-leaf execution assertion.
- The five area ledgers contain 172 feature-family records. All 217 top-level CLI
  candidates have an explicit classification and existing review owner. The 571 RPC
  identifiers remain discovery metadata, with source anchor bounds validated.
- Structured feature source references checked for existing repository files and
  in-bounds line numbers. Relative report links checked. These mechanical checks
  establish reference integrity, not semantic correctness; source review remains
  necessary and its limitations are described in each report.
- Changes are task documentation and identifier metadata only. No runtime code,
  dependencies, CI workflow or cloud deployment changed. No fresh macOS artifact,
  remote connection, transfer, browser permission journey or cloud sign-in was run.
- Independent read-only coverage review initially returned `fix-first` for omitted
  Computer Use, managed-device policy and project-pane contracts. Added those plus
  explicit quit lifecycle, repaired open-folder routing, and classified every discovered
  top-level candidate. Re-review returned `ship`, with no remaining material coverage,
  accuracy or status findings. Per-option/native/remote/production evidence remains open.
