# Work packages and commit boundaries

This is the normative implementation backlog derived from audit `554fd12`. Package IDs
are distinct from historical P0–P8 phases. Each package is multiple cohesive commits;
its listed slices are starting boundaries, not permission to combine an entire subsystem.
Retain working subsets and their tests. Every audit-row acceptance condition is inherited
in addition to the package gate. `closure-tracker.json` assigns all 172 families exactly once.
Secondary integration gates are stated below; another package cannot close a shared feature
on behalf of its primary owner. F01 also owns the 428 command/171 UI/217 candidate/571 RPC
catalog reconciliation and the shared CLI contracts, which are not additional family rows.

## F01 — Executable compatibility contracts and command registry

**Owner:** CLI/control: apps/cli, desktop-runtime/control. **Start dependencies:** F02.

1. Define original registry entries, argument/alias/output/error/focus schemas and availability states; migrate existing CLI without regressions
2. Generate grouped/leaf help, guide, capabilities and docs from executable definitions; add drift validation against frozen catalogs

**Exit gate:** Offline help works without app; every discovered public leaf has a contract or explicitly tracked unresolved contract; no help-only successful stubs; existing CLI retains intentional migration compatibility.

## F02 — Stable identities and shared action routing

**Owner:** workspace-core, desktop-runtime, native-bridge. **Start dependencies:** none.

1. Add stable object identities, typed refs/index resolution and caller-context precedence with model/ABI migration
2. Centralize target ownership, focus/no-focus, result/errors and lifecycle generations; migrate existing entrypoints

**Exit gate:** Wrong-kind, stale and cross-owner targets fail atomically; UUID/ref/index/legacy numeric forms resolve deterministically; UI and CLI share effects; remote routing cannot fall back locally.

## F03 — Effective capabilities and managed policy

**Owner:** settings/policy core, control auth, AppKit policy adapter. **Start dependencies:** F02.

1. Define capability and policy interfaces, credentials precedence and deny-by-default relay rules
2. Implement forced-preference resolution, settings locks and live client/resource revocation; wire each new owner as it lands

**Exit gate:** Config/env cannot bypass managed restrictions; ordinary defaults cannot impersonate MDM; malformed restrictive modes fail closed; policies cancel/revoke live access and preserve user settings on removal.

## F04 — Terminal engine decision and performance baseline

**Owner:** terminal-adapter, session-runtime, native harness. **Start dependencies:** none.

1. Measure current native idle/output/hidden-tab/browser workloads and reproduce reported hangs under controlled remote stalls
2. Evaluate current parser and permissively licensed alternatives against protocol/render/input fixtures; record architecture, license and numeric-budget decision

**Exit gate:** No engine choice from language preference alone; pinned license/provenance review and named Mac results; explicit gaps for graphics/keyboard/Unicode/reflow; bounded memory and latency budgets recorded before expanding limits.

## T01 — Terminal input, rendering and everyday interactions

**Owner:** terminal-adapter, session-runtime, TerminalView. **Start dependencies:** F02, F04.

1. Implement selected engine adapter and ordered PTY/resize/backpressure contract while preserving current working subsets
2. Complete selection/copy/paste, IME/key/mouse/scroll/find/clear/link behavior
3. Complete graphics/styles/font behavior and accessible terminal text with overload/error recovery

**Exit gate:** Production-owner protocol fixtures plus packaged Mac input/selection/scroll/IME/VoiceOver journeys pass; output floods and exited sessions remain inspectable without starving UI.

## T02 — Tabs, panes, focus and reversible close

**Owner:** workspace-core, native-bridge, WorkspaceViews. **Start dependencies:** F02.

1. Finish surface placement/reorder/split/focus actions and preserve live ownership across all entrypoints
2. Complete grouped close/reopen descriptors, original pane anchors, failure rollback and focus history

**Exit gate:** Cmd-T/W/Shift-Cmd-T, menu/context/CLI agree; no sibling restart or replayed input; close/reopen preserves supported layout and failures remain retryable.

## T03 — Multiple windows, groups and canvas layouts

**Owner:** workspace-core topology and native window host. **Start dependencies:** T02.

1. Introduce window ownership, cross-window moves, focus history and no-focus routing
2. Add workspace groups/metadata/status/reorder, pane equalize/zoom/auto-layout
3. Add canvas geometry/overview/tidy/align/distribute and shared persisted layout state

**Exit gate:** Native multiwindow operations preserve identities, sessions, unread ownership and focus; geometry/state is shared rather than view-cache-only; stale moves and capacity failures are atomic.

## R01 — Managed SSH with file/image drop and paste

**Owner:** new Rust remote/transfer owners, AppKit pasteboard adapter. **Start dependencies:** F02, F03.

1. Implement typed SSH destination/config/port/identity/proxy/forwarding/command descriptor and managed workspace creation
2. Implement bounded authenticated upload worker plus file/image/clipboard classification, progress/cancel and quoted remote-path insertion
3. Wire native drop/paste and CLI paste to the originating surface generation and connection; integrate local-file path behavior

**Exit gate:** Real two-host SSH and file/image drop/paste work together; verify host-key/auth failures, spaces/Unicode, jump hosts, disconnect and tab-switch/close during upload; no local fallback or wrong-terminal insertion. Does not wait for cloud deployment or full engine replacement.

## R02 — Durable remote sessions, relays and multiplexers

**Owner:** Rust remote/session owners and versioned remote daemon. **Start dependencies:** R01, S01.

1. Add daemon negotiation, authenticated scoped relay, retained output/replay, resize ownership and reconnect/cleanup
2. Add SSH-tmux and local tmux/zellij lifecycle; add mosh with explicit fallback semantics
3. Add remote-device routes and agent-session move after A03 supports transcript/checkpoint ownership

**Exit gate:** Reconnect and daemon restart preserve exact session identity without duplicate commands; remote spawn executes remotely; detach versus kill verified; missing tools/unsupported versions fail clearly; A03 is an additional gate for session move.

## N01 — Attention, notifications and sidebar UX

**Owner:** desktop-runtime attention, AppKit notifications/sidebar. **Start dependencies:** F02.

1. Finish scoped unread/read/open/dismiss/clear/jump and focused/background policy with bounded deduplication
2. Add OS notification permission/delivery/reply/actions and exact window/surface routing
3. Finish collapsed-sidebar access, resizable sidebar, keyboard/AX behavior and notification visual states

**Exit gate:** Hidden-terminal attention remains visible; normal output does not flood inbox; stale acknowledgements cannot clear new arrivals; denied OS permission preserves in-app operation; hook-produced attention verified again with A02.

## S01 — Session persistence, launch and quit recovery

**Owner:** new Rust persistence owner, native startup/termination. **Start dependencies:** F02, T02.

1. Version and atomically persist topology/restore descriptors; implement session save/open/export/import and corrupt-state recovery
2. Implement path/open startup, cwd/layout/command routing and crash/previous-launch restore
3. Unify quit confirmation, deferred cleanup, cancellation and recoverable snapshot; extend schema with each later surface/window kind

**Exit gate:** Restart/crash/corrupt files and failed restore retain recoverable data; exports omit secrets; restored local shells differ explicitly from preserved remote jobs; multiwindow/browser additions round-trip when T03/B01 land.

## S02 — Settings, shortcuts, palette and custom UI

**Owner:** settings schema/store, AppKit settings/palette/sidebar. **Start dependencies:** F01, F03, S01.

1. Add versioned config/schema/backup/validate/reload with last-good state and owned paths
2. Add shortcuts/chords/conditions, configurable menus/palette and focus-safe action dispatch
3. Add themes/import, sidebar templates/tools/configured buttons and content/browser settings as those owners arrive

**Exit gate:** Invalid config cannot partially apply; settings/env/policy precedence is consistent; every configurable action has an owner and disabled reason; browser/content settings are not complete until their runtime effects pass.

## B01 — Browser lifecycle, profiles and user workflows

**Owner:** BrowserView/WebKit bridge and Rust browser descriptors. **Start dependencies:** F02, F03, S01.

1. Complete routing/chrome/popups/history/zoom and persistent profile/private data ownership
2. Implement downloads, file chooser, dialogs, permission/auth/certificate UX and cancellation
3. Implement process-crash recovery, hidden-tab budgets, profile import/state and restoration

**Exit gate:** Real local HTTPS fixtures, denied permissions, downloads, profile isolation, closed-tab recovery and WebContent termination pass; no browser page obtains native socket authority.

## B02 — Full browser automation and developer workflows

**Owner:** versioned browser action bridge, CLI registry, WebKit execution. **Start dependencies:** F01, B01.

1. Implement generation-scoped DOM/accessibility snapshots/locators/get/eval/waits and semantic keyboard/pointer/form actions
2. Add frames/dialogs/files/screenshots/cookies/storage/state/scripts/console and REPL ownership
3. Add developer/focus/design/React Grab workflows and exact supported/unsupported capability outcomes

**Exit gate:** All audited browser leaves/aliases/options route to shared owners; close/navigation cancel pending work; cross-origin and public API limitations are explicit. Match deliberate unsupported outcomes at pinned cmux baseline; new unsupported gaps stay open.

## C01 — Content, project panes, file explorer and search

**Owner:** new content models/providers and native content hosts. **Start dependencies:** F02, S01, S02.

1. Add markdown/file preview/editor surfaces, atomic save/conflict handling and live reload
2. Add diff/review/comments/viewed-state and trusted sidecar lifecycle
3. Add local/remote file explorer and bounded cross-surface search; remote provider integrates R02
4. Add original project parser/pane for files/targets/schemes/build settings and project open routing

**Exit gate:** Each non-PTY surface has load/error/close/restore/focus/AX behavior; safe file routing and root ownership; no command injection; remote explorer remains incomplete until real remote acceptance.

## A01 — Events, metadata, inspection and tmux compatibility

**Owner:** desktop-runtime events/control and shared action owners. **Start dependencies:** F01, T02.

1. Add bounded ordered event stream/cursors/replay/reconnect and status/progress/log/PR/todo owners
2. Add offline/live shared rule matcher, rule logs and hook/signal/buffer/pipe/capture/respawn semantics
3. Complete current/tree/top/memory/health/diagnostics/RPC with process ownership and authorization

**Exit gate:** Slow consumers cannot stall UI; event gaps are explicit; metadata is target-scoped; exact-byte/exit/focus contracts verified; source-baseline unsupported tmux placeholders return honest failures, not fake success.

## A02 — Agent messages, hooks and attention integration

**Owner:** new agent adapters/message store plus attention/events. **Start dependencies:** A01, N01, S01.

1. Implement durable message/reply/inbox claim/lease/ack and wake policies
2. Implement idempotent provider hook install/remove/feed translation and completion/input-needed attention
3. Implement hibernation and feed/history UI with bounded retention

**Exit gate:** Agent messages are distinct from human notifications; duplicate hooks, lease expiry, concurrent delivery and stale targets behave deterministically; real supported provider events reach the exact surface.

## A03 — Agent launchers, recovery, accounts and integrations

**Owner:** agent launch/checkpoint/vault/provider adapters. **Start dependencies:** A02, S02.

1. Add typed launch/resume/composer/attachment ownership and provider version detection
2. Implement checkpoints/vault/recovery and team/external launcher wrappers
3. Implement CodeRouter/account integration and Glaeda exchange; integrate remote session move with R02

**Exit gate:** Actual supported external versions and absent-tool cases pass; arguments/credentials are safe; recovery evidence identifies the exact provider/session; failed remote move preserves source and destination integrity.

## H01 — Independent cloud identity and PR #1 requirements

**Owner:** services/control-plane, auth client, cloud settings. **Start dependencies:** F01, F03.

1. Complete native/web/CLI auth/team, session/member/client enforcement, MFA and localized mail
2. Complete org/admin/invite concurrency invariants, device presence/retention/revocation and session-bound ticket authority
3. Rehearse protected DB migration and deployment configuration; add GitHub Actions deployment after host/database decision

**Exit gate:** Every retained PR #1 matrix row has independent zmux evidence; real DB transactions and deployed callback/auth flows pass; origin remains zmux.w3dev.app; no raw-token handoff or operational upstream fallback.

## H02 — Cloud provider, VM lifecycle and entitlement control

**Owner:** control-plane provider adapters and infrastructure workflows. **Start dependencies:** H01.

1. Implement provider/image/artifact readiness, actor/team ownership and relay binding
2. Implement create/list/resources/resize/lifecycle with billing/entitlement admission and idempotency
3. Implement network/domain/publication access, snapshots/forks/restore/templates

**Exit gate:** Disposable provider environments prove lifecycle, ownership, quotas, webhook retries and cleanup; unavailable images/relays remain disabled. Provider/budget/host/DB/secrets are external decision gates; deploy only via Actions.

## H03 — Cloud terminal, development and transfer workflows

**Owner:** remote session client and cloud convenience orchestration. **Start dependencies:** H02, R02, B01.

1. Implement VM workspace/terminal/tab/layout/env plus encrypted secret delivery
2. Implement exec/run/route/agent/dev/push/pull/wait composition with bounded output and remote exit propagation
3. Implement desktop/TUI/ports/tools/handoff/access and remote browser integration

**Exit gate:** Each nested vm leaf and cloud alias has authenticated end-to-end evidence; no local execution fallback; secret handling, transfer cancellation and multi-client identity survive reconnect.

## X01 — Computer Use helper and approval lifecycle

**Owner:** independent signed helper/runtime, MCP adapters, native onboarding. **Start dependencies:** A03, F03.

1. License-review helper approach and implement signed helper identity, isolated credentials and readiness
2. Implement supported agent tool profiles/approval/snapshot targeting with per-launch injection
3. Implement TCC onboarding, settings/menu-bar/session projection and disable/replacement/revocation cleanup

**Exit gate:** Dedicated Mac verifies Accessibility/Screen Recording and client approval separately; passive refresh never prompts; stale clients/snapshots cannot act; release signing from D01 gates packaged acceptance.

## X02 — Mobile client, device access and VPN

**Owner:** iOS client, native extension and remote device transport. **Start dependencies:** H01, R02, F03.

1. Implement separate iOS app identity/callback/keychain and device session client
2. Add mobile terminal/browser controls, reconnect/background lifecycle and compatibility settings
3. Add separately approved signed VPN Network Extension with route/DNS/revoke/sleep-wake behavior

**Exit gate:** Real paired devices and signed extension pass ownership/revocation/permission tests; ordinary SSH remains independent of VPN; cloud-mobile scenarios additionally require H03; production signing gates closure.

## X03 — Simulator, capture, privilege and OS integrations

**Owner:** native platform adapters with shared registry. **Start dependencies:** F01, F03, T03.

1. Implement owned-window screenshot/recording artifacts and bounded capture lifecycle
2. Implement Simulator/iOS namespace workers, correlated inspector/actions and device selection
3. Implement signed privileged helper/Touch ID/requester checks and cancellation

**Exit gate:** Real devices/Simulator/capture bytes and signed-helper denial/approval cases pass; no success-shaped mocks; permissions are exercised only where the operation actually requires them.

## D01 — Polish, localization and trusted distribution

**Owner:** native design/AX, localization, release workflows. **Start dependencies:** F01.

1. Establish native visual/interaction baseline and reusable icon/control/empty/loading/error states; fix polish within every feature slice
2. Add catalogs and locale/AX review across native/CLI/service mail; finish welcome/feedback/update UX
3. Add signing/notarization/channel/update provenance and safe CLI install/upgrade/uninstall; retain Actions development artifacts

**Exit gate:** Final release gate depends on all user-facing packages; each intermediate artifact records SHA/tests/screenshots/checksum; Intel and Apple Silicon runtime, physical input/AX, signed update and moved-app CLI installation verified.

## V01 — Full compatibility and stability signoff

**Owner:** integration/acceptance owner. **Start dependencies:** F01, F02, F03, F04, T01, T02, T03, R01, R02, N01, S01, S02, B01, B02, C01, A01, A02, A03, H01, H02, H03, X01, X02, X03, D01.

1. Reconcile every frozen command/alias/option, UI action and public RPC/feature flag against executed evidence; resolve internal/unsupported classifications
2. Run combined native/remote/cloud/performance/recovery matrix, fix gaps and publish final Actions artifact and receipts

**Exit gate:** No unassigned, unverified or dependency-blocked required feature remains; only explicit user-approved scope changes may exclude behavior. Audit review is not runtime signoff.
