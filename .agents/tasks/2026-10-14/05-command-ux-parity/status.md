# Status

- **State:** in-progress
- **Updated:** 2026-10-15

## Active implementation

- R03 foreground local-zellij start/attach/list/status now uses kernel-verified
  ownership, a retained workload supervisor and shared child lifetime management.
  Independent Linux24 checks include real detach/reattach, input/resize and signal
  handling. FIFO blocking and SIGINT cancellation have red/green repairs. Parent
  combined fullcheck passes280 tests plus standard checks. Native production
  workflow is pending; ordinary native tabs and broader lifecycle parity remain.

- Reopen diagnostics now retain bounded public status/persistence/profile reads
  and the generated fixture session/allocator JSON, including failure artifacts.
  Isolated fullcheck271 and focused capture/receipt probes pass. This adds evidence
  for the next native run; it does not claim to repair the unresolved restore fault.

- Same-pane tab reordering now uses shared final-position planning, explicit
  focus (defaultfalse), CLI anchors/legacy syntax, native menu/shortcut/context
  actions and guarded same-pane drag. Isolated fullcheck passes271 Rust tests
  and standard checks; parent core/real-socket/focus/persistence checks pass.
  Swift/native execution and physical AppKit drag remain pending.
- Native `38794349738` at `9a30dec` passes universal build and final-close save
  with completed asynchronous shutdown. Reopen then stalls with an empty model;
  startup/restore diagnosis is active. Packaged acceptance remains failed.

- Darwin terminal artifact recipe independently passes17 integrity/real Clang
  Mach-O checks, source recipe verification and isolated full `scripts/check`
  (`/tmp/zmux-darwin-parent-check.log`). Manual workflow retains two thin macOS14
  slices, source/patches/notices and receipts; actual hosted build, inventory review
  and ARM64 runtime acceptance are pending. No production engine switch.

- Browser double-click, hover, DOM focus, check/uncheck and centered-scroll aliases
  now share admitted document-bound actions. Parent integrated `scripts/check`
  and all106 Chromium DOM cases pass. Browser startup-focus precondition repaired
  after diagnostics reproduced a missing focus event before document readiness;
  exact event assertions remain. Original opaque failure remains unattributable.
  Packaged WebKit execution is required and not yet claimed.

- Foundation `38794349737` at `9a30dec` passes Linux but exposes a macOS
  notification fixture snapshot `Busy` at line880. The suite now retries only
  snapshot publication contention through its existing bounded helper, and fails
  other snapshot errors immediately. Notification assertions and commands remain
  unchanged. Independent review accepts; integrated `scripts/check` passes
  (`/tmp/zmux-browser-notifications-integrated-check.log`). Hosted green pending.

- Native `38791991724` at `10b8f1b` passes universal compilation and reaches
  final-window termination. Its stack exposes a nested AppKit termination loop
  retaining the main actor before asynchronous shutdown can run. Termination is
  now scheduled through a run-loop selector; isolated `scripts/check` passes
  (`/tmp/zmux-termination-runloop-check.log`). Native confirmation is pending.
- Numbered workspace shortcuts/browser focus (`bb2b98c`) and reviewed static
  terminal artifact provenance/notices (`2360fb0`) pass isolated checks.
  Full parity remains incomplete; these are incremental implementation slices.

- `01b28e7` Foundation passes both hosts; universal native build passes, packaged
  startup again stops at initial makeKeyAndOrderFront. The bounded sample shows
  an idle AppKit run loop, not a blocking Rust/PTY stack. Retaining one menu tree
  per native window repairs repeated submenu reparenting on key callbacks; causal
  confirmation requires the next native run. Parent isolated full check passes.
- Browser click committed `8b66276`: 261 portable tests, strict checks, 57 registry
  probes and independent 62-case DOM check pass; native execution remains pending.
- PTY `38791084172` at `1abc98d` passes Linux and Darwin, including actual partial
  writes, cleanup and the two intentional failure controls. Production terminal
  engine selection is unchanged. Numbered workspace selection and remembered
  browser focus are under implementation/review; R03 foreground ownership is active.

- Workspace ordering committed `b4fcf6e`: parent full isolated `scripts/check`
  passes after repairing a prompt-sensitive cwd fixture with exact framed actual
  pwd output. Independent ordering and fixture amendment reviews pass. Native
  execution remains pending. Browser fill is committed separately as `0dc50f0`.
- Native `38787910086` universal build passes, but packaged startup stalls after
  bridge startup and before initial window activation completes. Per-call traces
  and bounded sampling of the exact isolated test executable are added; no timeout
  extension or second app launch. Zellij `38787914932` passes on Darwin at
  `11258fe`; PTY `38787910315` passes Linux but still fails Darwin, under diagnosis.

- Browser fill committed `0dc50f0`: exact 13 frozen source hashes, full isolated
  check (256 Rust tests, Clippy, notices and C ABI), independent review and parent
  Chromium 48-case DOM run pass. Google Chrome 151 failed to start the fixture;
  Chromium 155 completed it. Native WebKit execution remains pending.
- Workspace ordering is frozen against `0dc50f0`; independent review and parent
  isolated integration check are running. Native `38787910086` at `11258fe` passed
  Rust/C ABI and is building; pushes are held to preserve its lifecycle evidence.
  PTY `38787910315` and Zellij `38787914932` remain separate hosted gates.

- `c01c3bc` Foundation passes Linux and macOS (`38786746911`). Native
  `38786746932` failed strict Swift compilation at the zoom fixture's CGFloat
  string conversion; explicit Double conversion is repaired and isolated
  `scripts/check` passes again. The earlier window-lifecycle timeout remains
  unresolved; its new traces have not yet executed.
- PTY `38786746778` identifies Darwin cleanup deadline with master still open;
  master-before-wait repair passes independent focused Linux acceptance. Zellij
  `38787432280` confirms authentication repair but exposes short-read drain
  throttling in its TERM-output fixture; bounded waiter repair is in progress.

- Integrated workspace moves, corrected browser zoom and bounded native lifecycle
  diagnostics pass fresh `scripts/check`: 255 tests plus strict Clippy and C ABI.
  CLI moves default to no focus; native geometry follows workspace UUIDs.
  Zoom accepts finite factors and clamps to0.25–5. Same-window move semantics,
  configured-default zoom reset and exact-source native execution remain open.
- Hosted `16cf61a`: Foundation passes Linux/macOS; native universal build passes
  but packaged window close times out150s with empty logs. New phase diagnostics
  are prepared. Isolated PTY passes Linux, fails Darwin cleanup after reply/flood
  success; direct-child cleanup repair passes independent focused Linux checks.
  Zellij's real client-exit repair is being checked independently; an unrelated
  credential fixture rejection/send race is now reproduced and being repaired.

- Checkpoint `16cf61a` pushed: native lifecycle startup routing, process-group
  ownership cleanup, isolated kernel PTY acceptance and authenticated Zellij
  rejection fixture. Native `38784161310`, Foundation `38784161346`, and PTY
  `38784165438` are running. Zellij `38784160950` passed the repaired negative
  fixture but failed real attachment cancellation: terminal child did not exit
  successfully. Diagnosis continues; its Darwin gate remains open.
- Independent final Linux PTY acceptance passes frozen8/lock31, construction
  exhaustion cleanup and named negative controls. This is an isolated experiment,
  not a production terminal-engine switch. Workspace moves remain in progress.

- T03 two-window creation/focus/close, per-window rendering/attention and last-close
  save/reopen slice passes independent full portable checks and native source review.
  Packaged macOS execution remains required; moves/groups/canvas and full parity
  remain incomplete. Native3438565 failed an existing deadline-fixture ENOTCONN
  assertion; repaired without changing timeout or no-replay behavior.

- Local Docker service refreshed at `58a882b`: private backup, persistent migration002
  and expiry index verified, bounded cleanup codes0/tokens0, health/readiness200.
  Post-health `bunx path-to-port` confirms3530; tunnel/public auth and VM/relay/email
  remain open. See [local cloud](references/local-cloud.md).

- Latest checkpoint: Foundation `38778435301` passes at `c33ec40`; macOS
  `38778435343` passed universal compilation but failed save-panel destination
  selection in packaged smoke; fixture investigation continues.
  Full parity remains incomplete; no final parity artifact exists.
- F04 metadata hosted `38777353391` passes Linux/macOS; all29 inputs verified
  against `b86a700`, receipts recorded in `90f05e3`. Production engine unchanged.
- Zellij retained authenticated Quit passes independent real Linux rerun
  `/tmp/zmux-zellij-retained-parent-20261015/receipt.json`. Darwin short-lived
  kill-session credentials remain unsupported; repair committed/pushed `58a882b`,
  hosted `38779766351` confirms daemon exit and client exit0 but fails on59
  undeliverable opaque bytes during shutdown. Typed terminal-outcome handling is
  being implemented; no lossless shutdown claim.
- T03 two-window core/runtime/native and CLI are under validation, including
  close-before-open acknowledgement, physical capacity retention and key-window attention.
  Isolated real-PTY replies experiment continues separately.

- F04 isolated metadata owner committed/pushed `b86a700`; independent full39+7
  and final focused normal/source/arena controls pass, all29 final hashes checked.
  Hosted Linux/macOS `38777353391` running; no production engine switch.
- Native coordinator extraction `bded4e2` committed; downloads file-owner fixes
  passed hosted filesystem probe in `38776244720` at `eaf8cda`; app compilation
  and packaged WebKit acceptance still running. Earlier runs failed file-path
  handling then a Swift fixture isolation boundary; both retained strict guards.
- Zellij Darwin public SDK/peer and descendant containment now execute, but latest
  `38777058699` fails PTY failed-exec evidence and exact-close timeout. Diagnostics
  show exit111 with no retained PTY bytes, live server after helper exit0, and
  ENOTCONN relay closes. Owner is repairing per-direction lifetime and stage evidence.
- F01 contract schema frozen for independent review/parent checks. Two-window
  vertical lifecycle implementation and isolated real-PTY terminal design are active.

- Browser downloads committed `0bda3f1`: independent portable full checks/review pass;
  packaged Darwin file/WebKit acceptance remains pending. Zellij public-peer fixture
  committed `4d09a03`: independent real Linux transport/cleanup rerun passes;
  manual-only Darwin workflow awaits dispatch. Neither closes its full package.
- Persistence repair `610e897` passes Foundation `38773695057` and 128 fresh
  fixtures in diagnostic `38773735612`. Pre-fix also passed 128 attempts, so root
  cause remains unproven. Native `38773694848` passed universal compilation and packaged smoke; downloaded
  archive integrity/source/flags verified. Downloads run `38775130649` and Zellij
  public-peer run `38775157527` are active at pushed `d629406`.
- Next active slices: shared native coordinator extraction, opt-in bounded terminal
  metadata capture, and executable CLI compatibility contracts. Local Docker
  `/health` and `/ready` on port 3530 remain healthy; VM/relay/email remain unavailable.

- T03 singleton ownership committed8705dea and integrated terminal experiment
  committed93daf02; both pushed. Parent isolated full checks pass. Foundation
  38771928593 passes. macOS38771925429 fails before Swift compilation in the existing
  concurrent persistence lock test (ENOENT opening generation lock); focused owner
  investigation is active. Do not weaken lock outcomes or call this native acceptance.
- Integrated hosted experiment38771957372 passed Linux/macOS arm64; parent verified
  all23 input hashes against93daf02. Independent Linux rerun also accepted30 normal
  stages and five intentional assertion failures. Production engine remains unchanged.

- Native d6d9cc8 run38769334202 and Foundation38769333991 passed. Downloaded
  diagnostic archive/source/checksum and three images verified; profile manager,
  terminal keys/scrolling and durable tmux reopen now have hosted native receipts.
  Full V01 remains incomplete; no subset artifact is presented as final delivery.
- F04 resource/effects run38769399834 passed Linux/macOS; all17 input hashes
  verified against d6d9cc8. Single-owner composition is still under implementation.
- B01 downloads, T03 singleton window schema/durable migration, F04 integrated
  terminal ownership and R03 exact Zellij attachment design are active.

- B01 native profile manager committedbf1a4b3 after parent full scripts/check,
  foreign ABI checks and independent review. Create/list/open use the shared catalog;
  native manager execution and rename/clear/delete remain open.
- F04 isolated resource experiment committedf468782 and passed hosted38766300795
  on Linux x86_64 and macOS15.7.9 arm64; all14 downloaded input hashes match the
  commit. Mac raw universal compilation passed, but resource execution is arm64 only.
  Exact history/allocation-denial checks do not prove whole-app RSS or engine parity.
- Native528be92 andf4f99d3 both failed Swift nested async fixture cleanup compilation.
  The extracted-helper candidate1868dcc passed its standalone Mac diagnostic;
  full native38767602739 remains pending. Foundationf4f99d3 passed38765601911.
- R02b app-bound tmux attachment, reuse, persistence/reopen and typed caller routing
  pass full parent scripts/check and independent real-tmux/schema review. Model5 and
  sessionv3 retain old session readers. Native acceptance and remote tmux remain open.
- Browser profile presentation/menu and bounded browser runner committed/pushed528be92.
  Parent isolated scripts/check passed; nine subprocess lifecycle cases and35 Chromium
  DOM cases passed. Native38764217483 is pending; this repairs the missing-datastore
  Swift constructor exposed by38762433983, but native success is not yet established.
- Full V01 and its GitHub Actions macOS artifact remain the objective. Intermediate
  archives are diagnostic/acceptance evidence, not full-parity delivery.
- B01 explicit profiles committed94725f1 after207 portable tests, strict Clippy,
  notices/C ABI and independent review. Native profile isolation/reopen and relaunch
  remain open. R02b app attachment/restore is implementing against the new schema.
- Native948e922 passed universal build/packaged smoke in38759082621; archive checksum
  and exact source metadata verified. It predates B01 and file-backed key setup7035ee8.
  The intermittent multiline setup corruption remains open despite this pass.
- New cloud expired-credential cleanup passes12 PostgreSQL integration tests and fresh
  review in a disposable database. Persistent service database was not migrated/cleaned.
- Native5f5005e passes universal build and packaged smoke in38753451286: replies,
  sidebar pointer/keyboard resizing, metadata inspector, browser live title, special
  terminal keys and managed file-URL/refusal cases all executed successfully.
- Downloaded5f5005e ZIP checksum/integrity/source verified; AppKit and WebKit images
  inspected. Full-window compositor capture remains unavailable on hosted runner.
- B02b document locators/readiness committed/pushed6af571f. Parent full193 Rust tests,
  strict Clippy/notices/ABI plus35 real Chromium DOM cases pass. Foundation38754685035
  passed. Native38754684779 passed browser journeys but failed the later key fixture.
  Diagnostic38757351964 captured mangled Python input/SyntaxError with the intended
  terminal focused; input-path investigation is in progress, not a browser focus repair.
- F04 isolated engine feasibility passed Linux x86_64 and Mac arm64 execution plus
  both Mac architecture compile/lipo checks in38756429963. No production engine
  replacement or resource-budget claim. Native page allocation/history policy remains
  unresolved; an isolated compatibility exporter is in progress.
- R02 headless local tmux committed57c9df4; app-bound attachment/reopen/restore is in
  progress. B01 persistent browser profile binding is being validated independently.
- CI fixture isolation repair2435687 uses a process-local sequence as well as time/PID.
  Native38758505493 exposed a separate startup snapshot Busy race in the session test;
  the test now retries only Busy within its original deadline. Isolated scripts/check
  passes, including the real CLI session lifecycle, strict Clippy and C ABI.
- Local Docker cloud healthy at3530, confirmed with path-to-port after health checks.
  Public tunnel unverified; VM/relay/email unavailable. Hosted deployment remains deferred.

## Delivered slices

- R01d committed/pushed (`c260185`, compiler repair `ee8952f`): raw image paste/drop
  classification, bounded off-main PNG preparation with orientation, exact-session
  executor receipts and worker-finish source cleanup. Fresh reviews passed. Parent
  checks pass (79 Rust tests plus C ABI); universal compile and packaged native
  ImageIO/cache/pasteboard/view-to-shell smoke pass at `ee8952f` (`38720024342`).
  Diagnostic archive SHA/checksum and native render were inspected. Physical/remote
  image coordinator, directory/custom-transfer, stress and full R01/V01 remain open.
  See [receipt](references/r01-clipboard-images.md).


- R01c committed/pushed (`d7db8fe`), final fresh review passed: private authenticated terminal channels; bounded regular
  file uploads, progress/cancel and origin-bound insertion; native file drop/Finder paste.
  Local checks passed with 78 Rust tests plus C ABI. Hosted macOS exposed a Darwin poll
  incompatibility (`-1` descriptors); repair `c9cd45c` now passes Linux/macOS Foundation
  checks (`38716147517`) and universal/native packaged smoke (`38716147327`). Raw clipboard images, directories,
  two-host/physical/native acceptance and full R01/V01 remain open.
  See [receipt](references/r01-file-transfers.md).

- R01b committed/pushed (`ec9422e`), fresh review passed: public `zmux ssh`, validated wire routing, real SSH PTYs,
  no-focus creation results, endpoint-preserving tabs/splits/reopen without startup replay.
  Full local checks pass (73 Rust tests plus C ABI), including isolated real OpenSSH.
  Connection sharing, upload/drop/paste and full R01/V01 remain open.
  See [receipt](references/r01-ssh-workspaces.md).

- R01a descriptor foundation integrated: validated SSH endpoints/jump routes,
  host-key/config defaults, forwarding, bounded options and argv. Eight descriptor tests
  pass; full workspace checks pass (65 Rust tests plus C ABI). This established descriptors only; the R01b entry above records subsequent launch integration. Fresh review passed. See [receipt](references/r01-ssh-descriptor.md).

- F03a committed/pushed (`a1c621f`): local socket access/password modes, credential precedence
  and private-file validation, executor admission revocation and deny-by-default remote
  policy. Full checks pass (57 Rust tests plus C ABI); native/managed/resource gates
  remain open. Fresh repair review passed. See [receipt](references/f03-socket-authorization.md).

- F01a committed/pushed (`c572484`): shared command registry, grouped/task/leaf offline help,
  guide/skill alias, executable JSON catalog and app capabilities. Parser regressions
  for literal stdin markers and malformed browser help repaired; full checks pass
  (49 Rust tests plus C ABI). See [receipt](references/f01-command-registry.md).
  F01/full parity and native gates remain open.

- F02a committed/pushed (`d1393cb`): live UUIDs/typed refs/indices, scoped target resolution on
  the workspace executor, caller defaults, revision guards and real shell context.
  `scripts/check` passes (47 tests plus C ABI); fresh repair review passed. Native gates remain; F02 and
  whole parity are incomplete. See [receipt](references/f02-target-routing.md).

- Audit-driven closure plan prepared: 26 dependency-ordered packages cover all 172
  audited families, with commit slices, verification/artifact gates and first SSH/transfer
  delivery batch. See [execution plan](references/gap-closure-plan.md). Package execution
  remains planned; this update changes no runtime behavior.

- Broader whole-product source-family audit completed at cmux `3ba653b9` / zmux
  `69342a8`: CLI/global contracts, terminal/topology/native UX, browser/content/settings,
  agents/attention/automation/remote, cloud/platform/security/distribution. All 428 initial
  command rows and 171 shortcut rows have review routing; source-only namespaces and RPC
  declarations are separately cataloged. See [audit](references/full-parity-audit.md).
  This completes the requested broad audit, not implementation or per-option runtime proof.

- Corrective slice: terminal wheel/history navigation, typed-input return to live screen,
  worker-owned mouse protocol routing and exited-process history; focus/activation attention
  acknowledgement and Dock unread count. Local regression and workspace checks pass;
  macOS Actions compile and packaged smoke pass; artifact downloaded and verified.
  CLI repair backlog recorded in scroll-attention-repair.md.

- P1: independent terminal tabs; ⌘T/⌘W/⇧⌘T and Recently Closed; move/reorder, grouped closes,
  fresh-shell restoration and retryable launch failures. Hidden sessions remain alive.
- P2: collapsible sidebar with saved visibility, Workspaces/Notifications tabs, attention badges,
  bounded bell/OSC/explicit notification state and generation-safe read/open/dismiss/clear.
- P3a: embedded public WebKit surfaces without PTYs, address/navigation/find/zoom controls,
  retained hidden-tab state, CLI open/navigate, frame-owned split layout and ratio retention.
- Original MIT implementation; no imported or translated cmux GPL/BUSL code/assets.

## Verification and download

Current native runtime `ee8952f37dfe3dbe145e7423a66a7a1558da6dd4` passes universal
arm64/x86_64 compilation and packaged native rendering/image smoke (`38720024342`),
including 79 Rust tests plus C ABI in that job. Foundation Linux/macOS passed at
`c260185` (`38719421495`); the intervening repair changes only Swift failure literals
and task notes. Downloaded diagnostic ZIP passed SHA-256/integrity and source matching;
its native render was inspected. These are subset receipts; V01 remains false.

R01c runtime `c9cd45ca41ac256356329f7b7c2b668d3938ce48` passes all 78 Rust tests,
fmt/Clippy/notices/C ABI on Linux and macOS (`38716147517`), plus universal native
compilation and packaged rendering smoke (`38716147327`). The workflow archive is
diagnostic only; V01 is false and no new user-facing artifact is shared.

Current foundation evidence: `c572484` (run `38706071891`) and `a1c621f`
(run `38707343689`) passed macOS universal compile and packaged smoke. R01 descriptor `d06818a` passed Foundation checks (`38707919043`) and macOS
compile/packaged smoke (`38707919132`). R01b `ec9422e` passed Linux/macOS Foundation checks (`38711025578`) and
macOS universal/native smoke (`38711037026`).
These are diagnostic receipts; V01 remains false and no new artifact is shared.

The next artifact shared with the user must pass full feature parity (V01). Existing
links below are historical evidence for earlier subsets, not the requested new delivery.

Accepted runtime source: `70cd3b614e0b3f9cf0fcaa0e33bec4c59c228d7a`.
[Download macOS build](https://github.com/ashutoshpw/zmux/actions/runs/38692821187/artifacts/11952308942).
Local checks and hosted universal/native smoke pass; archive and both captured images inspected.
See [delivery receipt](references/native-delivery.md) for checksum, evidence boundaries and repairs.

## Open scope

- P0 executable per-leaf/option contracts, shared command registry, UUID/typed refs,
  caller defaults/auth implementation and measured Mac resource budgets remain open.
  The broad source comparison is complete; discovery and source review are not completion counts.
- Whole P1/P2/P3 phases remain incomplete: multiwindow/focus history/groups, original pane restore
  anchors, richer attention/hook/status operations, browser automation/profiles/downloads and
  remaining UX/command contracts are tracked in their references and phase plan.
- P4–P8 persistence/customization, automation/content tools, agents, remote/cloud and platform
  behavior remain open. Reopening creates fresh local shells or remote logins; job/session resurrection remains open.
- Interactive drag/IME/VoiceOver, Intel runtime, actual HTTP/TLS/permission journeys and numerical
  resource/remote-stress evidence remain open. Cloud callback smoke is not live cloud sign-in.
- User revised cloud hosting on 2026-10-15: hosted workflow deployment is postponed;
  run locally via Docker/PM2 and derive the service port with `bunx path-to-port`.
  Local Docker service is healthy at http://127.0.0.1:3530 with PostgreSQL over validated
  TLS on loopback port 3531. Parent checked /health, /ready and /v1/config. The user will
  map https://zmux.w3dev.app through Cloudflare Tunnel; public routing/auth remains unverified.
  VM, relay and email capabilities remain unavailable. Local setup changes await review.

## Next action

Validate the remote image paste coordinator and physical provider behavior, then
continue native file-URL/symlink/refusal and custom-upload contracts plus real two-host
transfer acceptance. Directory refusal and regular-file symlink semantics are already
covered by the existing R01 backend; recursive directory push belongs to H03. Continue remaining F02/F01/F03 contracts and the
F04 terminal-engine/Mac-budget evaluation alongside it. Preserve all working subsets.

User correction on 2026-10-15: incremental builds must not substitute for full source-based
parity. Prioritize the managed SSH + remote file/image transfer journey and complete owner/
effect/error tracing for every family. SSH is not blocked by cloud hosting. See the
[SSH/transfer comparison](references/ssh-transfer-contract.md). No runtime SSH support
was added in this comparison. The broader audit is now recorded in the reports above.

Implement the audited P0 command registry/target contracts and the managed SSH/transfer
priority journey, alongside remaining P1 navigation/window/recovery actions;
then close P2 attention and P3 browser gaps against the linked ledgers. Preserve native artifact
and visual gates, and commit cohesive validated slices. Full parity is still the objective.

## 2026-10-15 local hosting and native verification

- Local Docker launcher reviewed and environment override defect repaired; service healthy
  at http://127.0.0.1:3530. Hosted deployment deferred by user; public tunnel unverified.
- macOS Actions run 38725306919 at 85e6e9f passed universal build and packaged native
  smoke after shifted-bracket shortcut correction. No full-parity artifact handed off.
- Session persistence integration and crash-safe allocator work remain in progress.

## S01 foundation accepted locally

- Versioned private topology persistence, transactional empty-desktop restore, CLI receipts
  and native startup integration implemented. Durable allocator blocks prevent crash reuse.
- Review repaired stale OSC7 cwd persistence and silent final-save failure; independent
  runtime reproductions pass. Parent scripts/check: 123 tests plus Clippy/notices/C ABI.
- Native hosted checks pending; S01 additive/multiwindow/full-browser and safe cancel gates
  remain open. Next implementation begins B02a browser automation bridge.

## B02a shared browser bridge

- Browser navigation/read/eval/snapshot CLI now uses bounded native work receipts and
  host/document/restore fencing; shared menu/toolbar navigation enters the same owner.
- Parent 136 Rust tests, strict Clippy, notices and C ABI pass. Fresh reviews accepted
  shutdown cancellation, viewport and lost-event recovery repairs. Public native smoke
  authored; hosted Swift/WebKit execution pending. B01/B02 and V01 remain incomplete.
- Persistence Foundation/macOS checks at5f26fbe passed; no partial artifact handed off.

## Browser followup and N01a in progress

- `8ee0ee3` compiled universally on macOS; packaged browser smoke failed and Foundation
  exposed a temporary-directory collision. Neither run establishes full acceptance.
- No-focus browser creation and fixed-source CSP preflight are implemented; parent
  portable checks passed. Native followup roots its pending Promise, checks no replay
  across content worlds, and requires input progress before evaluation finishes.
- Local cloud health/readiness reverified HTTP 200; path-derived tunnel target remains
  `http://127.0.0.1:3530`. VM, relay and email remain unavailable.
- N01a native desktop notification ownership, permissions and exact-token actions are
  being implemented. In-app inbox behavior already exists; OS banners are not yet proven.

## N01a native attention integration

- Browser followup at `50309f9` passed Foundation and real macOS universal/packaged
  checks, including CSP/no-focus/pending-eval journeys. B02 advanced scope remains open.
- Native attention now captures epoch/namespace/session, routes confirmed exact actions,
  reconciles OS desires, and keeps denied permission separate from inbox access.
- Reviews repaired dropped restrictive permission observations and unbounded inbox
  action tasks. Native permission smoke runs its isolated runtime before the main app
  context, avoiding two owners of the same socket. OS banners remain a separate gate.
- Broader N01 CLI metadata, scoped operations and reply are not yet closed.

## N01b and transport verification

- N01b committed locally at `2619b56`: subtitle/defaults/desktop preference and scoped
  notification operations. Parent full check passes 161 tests plus Clippy/notices/ABI;
  fresh review accepted. Native UI and permission smoke await a green hosted run.
- `3ab1355` macOS runs exposed Darwin rejecting socket option updates after peer close
  while reply bytes remain buffered. A nonblocking, readiness-polled transport repair
  passes parent full Linux checks; hosted validation remains pending. No macOS notification acceptance is claimed yet.
- N01c typed workspace notification destinations are next. Full V01 remains open.

## Darwin repair and N01c

- `619a3eb` completes nonblocking/poll dependencies and peer-close regressions. Parent
  full Linux check and fresh review pass; Foundation38735209799 passes Linux/macOS.
  Native38735209748 passed Rust/ABI and is compiling; notification smoke still pending.
- N01c typed workspace destinations, CLI provenance and native inbox routing are in
  progress, with owner and public CLI regressions observed red before repair.
- Local Docker health/readiness rechecked HTTP200 at http://127.0.0.1:3530.

## N01c local verification and native crash diagnostics

- Typed workspace destinations and shared native routing pass 169 Rust tests, strict
  Clippy, notices and C ABI. Fresh production review accepted; external-caller smoke
  environment isolation repair is included before publication.
- `619a3eb` passed universal native compilation but packaged execution exited SIGTRAP
  before evidence. Empty stdout/stderr and absent immediate crash reports do not identify
  a cause. The workflow now captures a bounded LLDB crash backtrace; this diagnostic
  cannot satisfy the native smoke gate. Source-only actor-isolation guesses are unproven.
- N01d UUID handles and shared newest-openable-unread navigation are designed next.
  Replies, agent last-prompt events, richer unread state and full V01 remain open.

## N01d in progress

- `2dcb038` commits workspace notification ownership and failure-only native debugger
  diagnostics. Foundation38736550252 passes; macOS38736550307 is compiling.
- UUID handles preserve numeric compatibility; shared jump-to-unread will serve CLI,
  menu, inbox and Command-Shift-U with activation only after confirmed selection.
  Runtime/native integration and verification are in progress.

## N01d verification and confirmed native repair

- N01d full local check passes 172 tests plus strict Clippy/notices/C ABI. Public CLI
  handle/navigation cases pass; fresh production review accepted.
- `2dcb038` universal compilation passed but packaged smoke failed. LLDB proves the
  UN call-out queue hits a Swift main-actor assertion in refreshPermission's callback.
  Explicit Sendable completion boundaries repair the identified cause; next hosted
  execution must verify it. No partial artifact is shared.

## N01a-d hosted integration accepted

- `c9db69c` passes Foundation38739600484 and universal/packaged macOS38739600592,
  including permission ordering, metadata, workspace destination and unread navigation.
  The captured callback assertion is repaired on the actual native path.
- Downloaded archive checksum/integrity/source verified; AppKit and WebKit images
  inspected. Full-window compositor capture remains unavailable on the hosted runner.
- Local cloud HTTP health/readiness still pass at3530. Public zmux.w3dev.app did not
  resolve from this host on2026-10-15; tunnel setup remains external.
- N01e bounded PTY write receipts are in progress before reply integration. V01 stays
  open and no subset artifact has been handed off.

## N01e phase1 locally accepted

- Bounded PTY input receipts pass 175-test parent full verification and fresh race/
  cleanup review. Queued expiration cannot write later; active partial/unknown results
  never authorize replay. Complete establishes local PTY acceptance only.
- Next: immutable terminal reply opt-in and bounded core receipt polling, then native
  text-input category and confirmed outcomes. Native/remote reply parity stays open.

## Current integration at13504c3

- Full local checks pass after A01 integration and deadline-fixture correction.
- df3f8ff Foundation38749608190 passed; macOS38749608349 failed during a CLI test
  before native compile. The test incorrectly required body-phase expiry, although
  header-phase expiry under load also preserves the same absolute deadline contract.
- At13504c3 the test accepts either response phase, explicitly requires deadline expiry,
  and retains original timing limits, ordinary success, empty-output and no-replay checks.
- macOS38750257118 must establish actual metadata/title/sidebar/key execution; it is
  not yet an acceptance receipt. Browser readiness/locators and native R01 file-URL
  journeys are in progress. Engine evaluation is gathering source-backed alternatives.
- Local cloud health and readiness rechecked HTTP200; path-to-port again returned3530
  after health. Public Cloudflare Tunnel and VM/relay/email remain unverified/unavailable.

## Native diagnostic at700648d

- macOS38751027370 passes universal compile and reaches packaged smoke. The sidebar
  trace proves width/preference264 and correct hit routing, but AppKit promotes the
  divider to first responder on mouse-down. An event-specific focus repair is in progress;
  keyboard/AX focus and strict assertions remain required.
- First Darwin sampler artifact downloaded and inspected: two startup RSS samples,
  one successful status probe and one owner-unverified skip. This verifies instrumentation,
  not a performance budget; see F04 resource reference.
- R01 native file-URL/refusal acceptance committed004c05a after fresh review and full
  scripts/check in an isolated index snapshot. Native execution pending; not pushed yet.

## 5f5005e native acceptance

Run38753451286 produced a valid universal archive with SHA256
637944de020a31c9f337a28f2fb3125a6c928069512a305f8e4e17c1710081a6.
The parent verified archive integrity, source metadata, all required smoke flags and
both captured images.15 resource/CLI samples cover18.50 seconds of this mixed smoke;
these are instrumentation receipts, not controlled workload performance budgets.
Full-window screencapture status1 remains explicit. No subset artifact was handed off.
