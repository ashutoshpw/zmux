# Status

- **State:** in-progress
- **Updated:** 2026-10-15

## Delivered slices

- R01c committed/pushed (`d7db8fe`), final fresh review passed: private authenticated terminal channels; bounded regular
  file uploads, progress/cancel and origin-bound insertion; native file drop/Finder paste.
  Local checks passed with 78 Rust tests plus C ABI. Hosted macOS exposed a Darwin poll
  incompatibility (`-1` descriptors); repair and exact-SHA validation are in progress. Raw clipboard images, directories,
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
- Cloud is not deployed. Origin is https://zmux.w3dev.app; provider/database selection is deferred.
  All deployments must use GitHub Actions.

## Next action

Repair R01c Darwin polling, review/push and verify exact-SHA Linux/macOS/native checks. Then implement raw clipboard image preparation and
cleanup, directory/custom-upload contracts and real two-host/native transfer acceptance. Continue remaining F02/F01/F03 contracts and the
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
