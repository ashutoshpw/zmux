# Status

- **State:** in-progress
- **Updated:** 2026-10-15

## Active implementation

- User requires continued implementation through full V01 and a GitHub Actions macOS
  artifact; phase completion is not task completion. New implementation/review delegates
  use requested `gpt-6.1-sol` at high effort. Runtime model telemetry is unavailable.
- Working changes: source-baseline regular-file symlink transfers and explicit nonrecursive
  directory refusal; shared directional split/navigation and bounded focus history.
  CLI/native integration and original split restoration are implemented; parent full checks
  pass (98 Rust tests plus C ABI). Navigation review passed; remote-image harness review
  found file-publication and SSH-child cleanup races, repaired with atomic publication and
  owned-process-tree cleanup. Fresh repair review passed. Native Actions execution is
  pending; V01 remains open.

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
continue directory/symlink/custom-upload contracts and real two-host/native transfer acceptance. Continue remaining F02/F01/F03 contracts and the
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
