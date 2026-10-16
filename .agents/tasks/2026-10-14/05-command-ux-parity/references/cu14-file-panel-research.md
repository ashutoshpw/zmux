# CU-14 / CV-01 / CV-05 file panel: codebase research

Status: research input for the `zmux open` + file preview/editor panel slice. Written 2026-10-16 from a
read-only code survey of this repository. Original Zmux code is MIT; no cmux source, tests or assets
were consulted for this document or may be consulted for the implementation.

## Problem

`zmux open <file>` fails with ``unknown command `open` ``. `apps/cli/src/main.rs` has no `open` arm and
`apps/cli/src/registry.rs` has no entry. `CMD-006 open` is `unresolved` / `local_contract: null` in
`apps/cli/src/registry/frozen-coverage.json`. The destinations it needs are also missing: CV-01
(Markdown panel), CV-05 (file preview), CV-06 (atomic save/conflict), CU-14 (mixed file/dir/URL router).
Only `zmux browser open [PANE] URL` exists, and the browser path rejects `file:` URLs
(`validate_browser_url`, `crates/workspace-core/src/surfaces.rs:1151`; Swift `BrowserView.allowed()`).

## Required observable behaviour (functional spec, not copied)

`zmux open <path-or-url>...` classifies each operand safely and reports a per-item result:
files open as tabs in the target pane (several files become several tabs in the same pane); Markdown
files open in a rendered Markdown view; other text files open in an editor; image/PDF open in a preview
mode; http(s) URLs reuse the existing browser creation action; unsupported items (directories, binaries,
oversize, unreadable) produce an explicit per-item error, never a success-shaped placeholder.
Target selection: `--window/--workspace/--pane/--surface` (+ `ZMUX_WORKSPACE_ID` / `ZMUX_SURFACE_ID`
defaults via the existing `targets.rs` rules), `--focus true|false` / `--no-focus`. Relative paths are
made absolute in the CLI. The panel is an editor "similar to the browser" panel: a tab in a pane, closable,
reopenable, restored across sessions, with dirty state, save (Cmd-S), conflict handling and live reload.

## Rust model (workspace-core)

- `Surface` is one flat struct (`crates/workspace-core/src/surfaces.rs:5-31`), `SurfaceKind { Terminal, Browser }`
  (snake_case on the wire). Terminal built by `Surface::terminal` (:21); browser built inline in
  `CreateBrowser` (:194-230). Construction sites to update for new fields: `Surface::terminal`, the browser
  literal, `fresh_surface` (:654-661, used by reopen), restore literals in `restore.rs` (~648, ~666).
- `Action` enum `crates/workspace-core/src/lib.rs:216-367` (`tag = "action"`, snake_case, `deny_unknown_fields`).
  `apply` (:463) clones the store, applies, validates, swaps; rejected actions leave the store untouched.
  `validate_terminal_descriptors` (:486, `local_zellij.rs:299-336`) rejects cross-kind identity; a new kind
  needs an equivalent exclusivity rule.
- `ObserveBrowser` is trusted-host-only: `Command::Action{ObserveBrowser}` is rejected at
  `crates/desktop-runtime/src/lib.rs:1574-1579`; it arrives via `BrowserHostEvent` (:1535-1546). Dirty-state
  observation for files should copy this trust model.
- Limits: `MAX_SURFACES_PER_PANE=32`, `MAX_CLOSED_ITEMS=32`, `MAX_NAME_CHARS=128`; `MAX_TERMINALS=8`,
  `MAX_BROWSERS=4` in desktop-runtime (:64-65, enforced :2236-2248, and in `RestoreLimits`,
  `restore.rs:237`, fed from `sessions.rs:90-96`). A new kind needs its own cap in both places.
- `MODEL_VERSION = 6` (`lib.rs:50`); Swift hard-checks `== 6` at `apps/macos/Sources/NativeBridge.swift:133`.
  Bump both together. `SESSION_DOCUMENT_VERSION = 9` (`restore.rs:15`); versions are explicitly gated;
  `SavedContent` (:138-162) gets a new variant gated to the new version. Capture strips browser URL
  query/fragment for privacy; a saved absolute file path is more sensitive: decide and document.
- Closed-item history stores full `Surface` clones and is not persisted; unsaved editor text is not stored in
  Rust, so closing/reopening a dirty file needs an explicit policy (refuse/confirm close while dirty; reopen
  reloads from disk).
- `workspace-core` performs no I/O at all (`lib.rs:1`). File I/O belongs in `desktop-runtime`.
- Targeted CLI actions: `TargetAction::CreateBrowser` (`workspace-core/src/targets.rs:~643`, resolved ~749-762)
  becomes `Action::CreateBrowser`; the desktop resolves the profile before resolving (`desktop-runtime/src/lib.rs:1695-1706`).
- Other kind-aware sites: `inspection.rs:107-125`, `agent_messages/domain.rs:156` (already terminal-only),
  `downloads.rs`, `control.rs` / `control_policy.rs` (`is_read` is an exhaustive match, `control_policy.rs:240`).

## Desktop runtime (executor and workers)

- `Desktop::submit` -> bounded `sync_channel(MAX_COMMANDS=32)` -> single worker `zmux-workspaces`, 16 ms tick
  (`desktop-runtime/src/lib.rs`). No blocking I/O on this worker; PTY launches use spawned threads reporting
  through a bounded completion channel; persistence has its own worker (`MAX_PENDING_OPERATIONS=8`,
  `MAX_DOCUMENT_BYTES=512 KiB`). Shutdown must join workers within the deadline (`shutdown.rs`).
- Browser runtime precedent (`browser.rs`): bounded pending ops (8), receipts (16), fenced by host UUID,
  restore epoch, model navigation and document generation, retire-on-close in `reconcile`. File ops need an
  equivalent fence so a stale save from a closed/replaced view is rejected. Never auto-replay after an
  unknown outcome.
- Reusable file-safety code: `persistence.rs` (absolute path, `openat` component walk with
  `O_NOFOLLOW|O_DIRECTORY`, `O_NONBLOCK` + regular-file check, `read_bounded`, `atomic_write` via same-dir temp
  `O_EXCL|O_NOFOLLOW` + `fsync` + `renameat` + dir sync). Its owner/0600 mode checks are for private state
  and MUST NOT be reused for user files. `download_paths.rs` is the closest bounded poll-based observer
  (single worker, 16-slot queue, 50 ms `fs::metadata` poll). `transfers.rs` / `upload.rs`: absolute UTF-8 paths
  without control chars; 64 MiB per file. There is no file watcher (no `notify`/inotify/FSEvents) anywhere:
  external-change detection is bounded polling of a fingerprint (dev, ino, size, mtime_ns, content hash) on
  the file worker. New third-party crates require `docs/dependencies.md` + `scripts/license-inventory` +
  `THIRD_PARTY_NOTICES.md` updates; prefer none.
- Symlink policy: existing code rejects symlinks in every path component; a user editor must decide (e.g.
  canonicalize once at open, then pin and re-verify), because macOS `/var` -> `/private/var` style links are normal.

## Native bridge

- Hand-written C header `crates/native-bridge/include/zmux.h`; docs `docs/native-bridge.md`; `ABI_VERSION=1`.
  JSON envelope `{"version":1,"request_id":N,"payload":{...}}`; limits: 16 KiB request, 8 MiB response.
  Swift reads snapshots by polling `zmux_desktop_snapshot(handle, revision)` (snapshot JSON = derived `Serialize`,
  so new model fields flow automatically). File bytes/editor text must never travel in `Snapshot`.
- Trusted native-only export pattern (`zmux_desktop_download`, `lib.rs:~683-707`): bounded request,
  `Reservation::new()` taken first, 2 s wait, receipt, `outcome_unknown`, "off UI thread, never replay".
  A file read/write export follows this; a save payload can exceed 16 KiB so it needs its own explicit
  documented limit rather than going through `dispatch`.
- Checks: `scripts/check-native-bridge` compiles `crates/native-bridge/tests/abi.c` and asserts exact JSON replies.

## CLI and control socket

- `apps/cli/src/main.rs`: `enum Parsed` (:71), `parse` (:237), targeted commands route to `targets::parse`
  (:405-409), simple ones are inline arms (`new-workspace` :477). `registry.rs` `command!` entries drive help,
  `zmux commands`, parser routing (`targeted` flag!). `registry/contracts.rs` `definitions()` (:134) holds bounded
  contract leaves; `validated()` (:1188) runs each leaf's probe through the real `crate::parse` and only accepts
  `Parsed::{AgentMessages,Buffers,PullRequest,App,WorkspaceMove,SurfaceOrder,Browser,BrowserDownload}`: a new
  `Parsed` variant must also be allowed there. `frozen-coverage.json` entry for `CMD-006` moves to
  `status: "local_counterpart_compatibility_unverified"`, `local_contract: "open"` only if a leaf `open` exists;
  never set behavioural parity true. `apps/cli/contracts/baseline/*` is frozen (SHA-256 recorded): do not edit.
- Wire: `crates/desktop-runtime/src/control.rs` (`ControlCommand` tag `method`, `deny_unknown_fields`, 16 KiB request,
  1 MiB response, `CONTROL_VERSION=2`). `targets.rs` in the CLI parses selectors only; the server resolves them.
  `paste-files` (`apps/cli/src/targets.rs:232-253`) is the model for path operands made absolute against
  `env::current_dir()`. `browser open` (`browser.rs:102-106`, `targets.rs` `extract_browser_focus` :493) is the model
  for focus flags and `created_surfaces` receipts. A 16 KiB request cap bounds the number of paths per request.
- Tests to extend: `apps/cli/tests/cli.rs` (`help_and_version_are_complete_and_offline` :70 command list;
  `capture_request` :349 wire-capture pattern; catalog/contract assertions ~:175; real-Desktop end-to-end :850),
  `apps/cli/tests/browser.rs` (:947 creation receipts/focus), `apps/cli/tests/ssh.rs` (paste-files path rules),
  `apps/cli/tests/windows.rs` (`Fixture` with real Desktop + ControlServer). Tests must `env_remove`
  `ZMUX_WORKSPACE_ID` / `ZMUX_SURFACE_ID` / `ZMUX_TAB_ID`. `help_and_version...` count assertions: inventory stays 428.
- Reference commits for shape: red `test:` commit then `feat:` commit (`e178d25` -> `ffc635e` workspace groups).

## Native macOS app (apps/macos/Sources, Swift 6, no Swift toolchain on this Linux machine)

- `MainWindowHost` (`App.swift:63`), `paneView` (`App.swift:942-1011`) dispatches by surface: `sessionId` -> terminal,
  `localZellijCompletion` -> `CompletedTerminalHistoryView`, **everything else is assumed to be a browser** (trap).
  `TerminalTabControl` icon is hard-coded `kind == "browser" ? "globe" : "terminal"` (`WorkspaceViews.swift:643`).
  Other `kind == "browser"` sites: `ApplicationCoordinator.swift` (~199, 405, 424, 486), `App.swift` (850, 897),
  header `updateHeader`/`terminalTitle` (`App.swift:826-872`), close-confirmation copy (`App.swift:1310`).
- Models: `Models.swift:208` `Surface` (kind: String + optionals), `PaneTree.key` (:226) drives layout invalidation.
- Lifecycle: views are retained in coordinator dictionaries (`browsers` etc., `ApplicationCoordinator.swift:95-98`),
  created/pruned in `reconcile` (:380-501), re-parented by windows. `BrowserView.update(_:)` reloads only when
  `surface.navigation` changes (generation fence). `BrowserCommandOwner` is the narrow callback protocol.
- Bridge: `actor NativeBridge` (`NativeBridge.swift:59`), `poll()` :123, `action(...)` :169, `browser(...)` :183,
  `send()` retries BUSY. `UICommand` enum + bounded 256 `AsyncStream` + `commandTask` switch
  (`ApplicationCoordinator.swift:4-31, 137-192, 349`). Add UICommand cases for file open/save/observe.
- Focus: `completePresentation` (`App.swift:704-731`) and `releaseContentResponder` (:178) need a branch; follow
  `CompletedOutputTextView.onFocus` -> `focus_pane` pattern (`App.swift:965-967`).
- Existing text pattern: `CompletedTerminalHistoryView.swift:5-86` (NSTextView in NSScrollView, monospaced,
  accessibility label). No PDFKit/QuickLook/Markdown code exists. Prefer system frameworks (NSTextView,
  `NSAttributedString(markdown:)`, PDFKit `PDFView`, `NSImageView`) so no third-party notices are needed.
  A WKWebView for Markdown must be a separate configuration from `BrowserView`.
- Menus: `makeMenus()` (`App.swift:563-682`) tuples; Edit routes through responder chain; Cmd-1..9 reserved,
  Browser menu uses Cmd-R / Cmd-F / Cmd-L; `validateMenuItem` (:1363). Window `SidebarFocusWindow.performKeyEquivalent`.
- Tests: no XCTest/Package.swift. Behaviour is covered by in-app smoke enums (`SurfaceOrderSmoke.swift` is the template,
  `CompletedTerminalHistorySmoke.swift` the closest non-PTY precedent) wired from `AppDelegate`/`runSmokeTest`
  (`App.swift:2668-2680`, `3004+`). Standalone Swift logic can be tested with `tools/native-fixtures/*` +
  a `scripts/check-*` script (macOS only). All Swift gates (`scripts/check-macos-sources`, `build-macos-dev`,
  `check-macos-app`, smokes) require macOS and run only in `.github/workflows/macos-development.yml` /
  `check.yml` (macos-latest). Record them as unavailable locally; CI is the verifier.

## Validation and delivery rules

- `scripts/check` = `cargo fmt --all -- --check`, `cargo test --workspace --locked`,
  `cargo clippy --workspace --all-targets --locked -- -D warnings`, `scripts/license-inventory --check`,
  `scripts/check-native-bridge`. All run on Linux. No commit hooks exist; never bypass anything.
- Commit style: lowercase conventional commits (`test:`, `feat(cli):`, `fix:`, `docs:`), cohesive validated slices.
- Task docs to update with evidence: `references/closure-tracker.json` (package C01/S01, features CU-14, CV-01, CV-05,
  `delivery_status`, `implementation_commits`, `verification_receipts`, `remaining_gates`; subset delivery is
  `implemented-unverified`, never `verified`), `status.md` (Active implementation, newest first), a receipt
  `references/cu14-file-panel.md`, `apps/cli/src/registry/frozen-coverage.json`, `docs/native-bridge.md`,
  `docs/local-control.md`, `docs/dependencies.md` only if a dependency is added. Do NOT edit
  `apps/cli/contracts/baseline/*`, the historical `command-inventory.json` / `audit-*.json|md` copies.
- Native/hosted gates unavailable on Linux must be recorded as unavailable, not passed.
- `docs/dependencies.md` states implementation and fixtures are authored for zmux with no cmux source imported.
