# Status

- **State:** in-progress
- **Updated:** 2026-10-15

## Active implementation

- Full V01 and its GitHub Actions macOS artifact remain the objective; subset builds
  are diagnostic only. Delegation uses the requested gpt-6.1-sol model.
- N01a-d notification integration and the callback-isolation crash repair passed
  real macOS packaged checks at c9db69c. Session write receipts also pass Linux/Darwin
  checks and native packaging at6cea10a.
- N01e reply journey executed successfully at3a9ab8e in macOS38745262578;
  that run failed later on live browser titles. Actual OS text-action interaction remains open.
- Browser owner fencing and bounded live-title KVO repair are committed; native title
  regression was observed before repair. Rerun is blocked earlier by sidebar acceptance.
- Sidebar resizing compiles universally but macOS38747700382 failed the compound
  drag/focus check. Diagnostic-only df3f8ff adds width, event-hit and responder evidence;
  macOS38749608349 stopped earlier on a scheduler-dependent CLI test assertion.
  That assertion is repaired without changing runtime/timing; macOS38750257118 at13504c3
  is the next diagnostic run. No speculative sidebar production repair is claimed.
- T01a F1–F12/modified navigation encoder is committed; native PTY-byte acceptance
  has not yet executed because of the earlier sidebar failure.
- F04 bounded external resource sampler committed at2095b5b and pushed with df3f8ff;
  portable review/checks pass, first Darwin measurements pending.
- A01 workspace status/progress/log plus native rows/inspector committed at035a285;
  icon spacing repaired at13504c3. Full local checks (189 tests) and independent source
  reviews pass. Native compilation and execution remain pending.
- Local Docker cloud is healthy on3530; `bunx path-to-port` confirmed3530 after
  health checks. Public tunnel is unverified; VM/relay/email remain unavailable.

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
