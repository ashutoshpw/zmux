# Audit-driven implementation and delivery plan

Date: 2026-10-15. Audit baseline: commit `554fd12`; cmux source stays frozen at
`3ba653b9e5afb11d0bc578031142950d6136697e`. This plan supplements the original
P0–P8 scope with executable work packages and supersedes its implied serial ordering.
It does not restart already delivered tabs, scrollback, notification or browser work.

## Outcome and architecture

Close every audited behavior gap with original MIT code and dependency notices. Keep
Rust as the authoritative workspace/session/control/persistence owner and the existing
Swift/AppKit/WebKit native presentation boundary. Reopening the UI-stack decision is
not a prerequisite. Terminal engine capability is a measured decision in F04; extending
the current feasibility parser indefinitely is not assumed to be the solution.

Public CLI, native UI, shortcuts, menus, palette and authorized remote entrypoints call
shared typed actions. Workers own PTY, network, transfer and filesystem waits. Stable
identities and generation tokens bind asynchronous effects to their original targets.
All histories, queues, messages, output and pending requests have defined bounds and
visible overload/recovery behavior. Every new subsystem honors effective capability,
managed-policy and object-ownership checks.

The [26 work packages](closure-work-packages.md) identify module responsibility,
prerequisites, commit boundaries and exit gates. The [closure tracker](closure-tracker.json)
assigns **all 172 audited feature families** a primary package, preserving their source
status and acceptance criteria. An implemented subset is neither discarded nor marked
complete; it is extended and reverified against its full contract.

## Ordered delivery milestones

| Milestone | Packages / order | User-visible result and release gate |
| --- | --- | --- |
| M0 — shared foundation | F02 and F04 start; then F01 and F03; D01 visual/signing groundwork | Stable target routing, trustworthy offline help/capabilities and a measured engine/resource decision. Existing commands continue to work. Local socket authorization stays intact. |
| M1 — managed SSH journey | R01 after F02/F03; T01/T02 and N01 progress alongside it | `zmux ssh` opens an identified remote workspace; file/image drop and clipboard paste upload and insert into the correct remote terminal. Real two-host acceptance is mandatory. Local input, tabs and attention improve in the same development series. |
| M2 — dependable daily terminal | Finish T01/T02/N01; T03 and S01; then R02 core | Complete navigation/multiwindow/groups/canvas, reopen/restart/crash recovery, OS attention actions and reconnect/multiplexer behavior. SSH acceptance does not wait for every M2 feature. |
| M3 — browser and configuration | S02, B01, then B02 | Persistent profiles, downloads/dialogs, complete browser control and shared settings/shortcuts/palette. Native lifecycle, permission, data-isolation and browser automation gates pass. |
| M4 — content and automation | A01 may start after M0/T02; C01 after S01/S02 | Inspection/events/rules/metadata/tmux compatibility plus markdown/diff/review/editor/project/file/search surfaces. Remote file providers finish with R02. |
| M5 — agent workflows | A02, then A03; finish R02 session move; X01 | Real agent hooks/messages/recovery/team tools and separately permissioned Computer Use. Provider versions, authenticated clients, TCC and failure/recovery tested. |
| M6 — independent cloud | H01 can start after M0; H02/H03 require external decisions and remote dependencies | PR #1 auth/org/MFA/mail/admin requirements, deployed control plane and complete VM/relay/network/file/snapshot/dev workflows. Every deployed journey runs through Actions against owned infrastructure. |
| M7 — platform and release | X02/X03; complete D01 | iOS/mobile/VPN, Simulator/capture/privilege, localization, polished accessible UI, trusted updates and production distribution. Signing/device/permission dependencies remain explicit. |
| M8 — parity signoff | V01 after all applicable package gates | No unresolved required feature, option, alias, public entrypoint or dependency gate. Publish accepted macOS artifact and exact evidence matrix. |

Milestones are delivery groupings, not a mandate to serialize independent work. Prefer
closing the M1 SSH/transfer journey over beginning unrelated advanced features. F01
migrates existing commands first; each later package registers and implements its own
commands as one slice. Do not postpone useful command/help improvements until all
families are implemented, and do not advertise pending commands as working.

No calendar estimate is promised before F04 measurements and the remote/browser/helper
feasibility decisions. This is a multi-release program, not one large implementation
commit. Re-estimate milestone capacity after M0 using actual implementation throughput.

## First implementation batch

1. **F02a:** define identity and target-resolution migration plus shared result/error/focus
   contracts. Cover stale/wrong-kind/foreign targets and existing numeric CLI behavior.
2. **F01a:** register the existing implemented commands, offline task/leaf help and
   machine-readable availability. Freeze schemas/options/defaults for the SSH journey.
3. **F03a:** add capability/policy interfaces and scoped local/remote authorization
   contracts; no generic unscoped remote socket tunnel.
4. **R01a:** build managed SSH descriptors and launch/auth/error flow using safe argv,
   including ssh_config aliases, explicit port/identity, proxy/jump and focus behavior.
5. **R01b:** ship bounded file/image upload plus native drop/paste targeting, progress,
   cancellation and remote path insertion. Verify switch/close/disconnect races on a
   real second host before calling the journey usable.

Run F04 measurements and engine evaluation alongside this batch. Engine replacement
must not block managed SSH metadata/transfer work, but T01 protocol/render completion
requires that decision. Publish a development artifact for each coherent integrated
runtime batch with clear implemented and pending capabilities.

## Contracts and coverage enforcement

F01 converts discovery into an original executable registry. Each leaf has canonical
name/aliases, options/defaults/constraints, target kinds and caller precedence, focus
effect, output/error/exit schema, capability/policy requirements, dependency versions,
owner and behavioral verification references. Dynamic domains (selectors, indices,
settings keys, provider arguments) have validators, not invented finite leaf counts.

Cover the original 428 command rows and 171 UI actions plus source-only namespaces and
571 RPC identifiers. Classify internal helpers, feature-flag/debug-only methods and
intentionally unsupported baseline behavior explicitly. Advertised capability is never
authorization. When cmux itself returns `not_supported`, a matching documented failure
can satisfy that specific contract; a zmux-only limitation cannot silently remove scope.

Use the tracker states `planned`, `implementing`, `implemented-unverified`,
`dependency-blocked`, and `verified`. A verified family needs implementation commits,
production-owner checks, applicable native/remote/deployed receipts and no unresolved
audit acceptance gates. Update individual command contracts too; family completion
cannot hide a missing leaf. Record a scope exception only when explicitly approved by
the user. Baseline changes require a separate diff review; do not move the target quietly.

## Work ownership and integration

These are responsibility boundaries for future implementation, not agents dispatched
by this planning turn. The integration owner controls shared Action, Snapshot, ID and
C ABI changes. Feature owners request schema changes before editing consumers. Version
contracts first and integrate dependent code against that revision; avoid two workers
rewriting `App.swift`, the CLI parser or the core model simultaneously.

Native polish and accessibility belong in every feature's acceptance: title follows the
active surface, lowercase `zmux`, coherent icon-only controls with tooltips/AX names,
keyboard focus, selection, spacing, contrast, loading/empty/error states and all sidebar
states. D01 owns shared standards and final cross-feature review, not a deferred cosmetic
pass. Keep shell/agent activity distinct from attention and from transfer progress.

Each commit is a compilable behavior slice spanning its owner and necessary consumers,
with appropriate regression coverage and task evidence. Follow the package's proposed
slice boundaries, split further when review becomes unwieldy, and avoid file-by-file
microcommits. Retain user/unrelated edits; do not revert another owner's work. Commit on
`main` after `scripts/check`; use the repository worktree rules if a branch is needed.

## Verification and artifacts

User steering on 2026-10-15: share the next artifact only after full feature parity
is verified at V01. Intermediate Actions archives support diagnosis/acceptance work;
they are not the requested delivery. Do not substitute a partial build or audit review
for the applicable native, two-host, deployed-service and stability gates.

- Test observable behavior at the owning layer. Cover invalid/stale targets, focus,
  cancellation, duplicate/retried events, permission denial, disconnect, restore and
  bounded overload as relevant; do not count parser tests as feature implementation.
- Use native macOS fixtures and packaged smoke for each UI/runtime slice. Add physical
  trackpad/IME/VoiceOver and Intel runtime evidence where synthetic/Apple Silicon checks
  cannot establish the claim. Compare approved visual states, including narrow windows.
- Run two-host SSH/transfer/relay and real browser/provider fixtures for dependent
  journeys. No credentials in fixtures, logs, exports or artifacts. Cloud mocks validate
  contracts but cannot close deployed-service gates.
- F04 records named hardware/OS/build, idle/1/8-session CPU/RSS, p95 input/main-loop
  delay, output fairness, hidden-browser cost and cleanup. Define numeric thresholds
  from that evidence; V01 enforces them with sustained output and multi-hour remote
  reconnect/stall tests. Do not claim a new stack fixed hangs without reproducer evidence.
- GitHub Actions produces downloadable macOS development artifacts for runtime batches:
  exact SHA, universal build, executed smoke/tests, archive checksum, screenshots and
  expiry/download URL. A packaged build remains provisional until applicable native and
  journey gates pass. Documentation-only planning changes do not need a new app build.

## External decisions and retained scope

| Decision/dependency | Blocks | Work that continues |
| --- | --- | --- |
| Cloud hosting/database/provider/budget and deployment credentials | Live H01–H03 deployment/provider acceptance | Local core/CLI/UI/SSH/transfers; auth contracts, disposable DB tests, provider interfaces and Actions definitions |
| Developer signing/notarization and helper/extension identities | Production D01, signed X01/X02/X03 acceptance | Development artifacts, core adapters and controlled fixtures; never substitute an ad hoc helper for real TCC evidence |
| Dedicated Mac, Intel runtime and physical/paired devices | Relevant native/AX/mobile/performance gates | Portable tests, synthetic smoke, source implementation; keep dependent rows unverified |
| Supported provider CLIs/versions and legitimate service credentials | A03 external integration and real cloud journeys | Safe argument forwarding, missing-tool paths and isolated adapter tests |

Cloud origin is `https://zmux.w3dev.app`. All deployment/migration execution is through
GitHub Actions. Do not choose a hosting/database provider, provision paid infrastructure
or treat missing approval/credentials as supplied by elapsed time. Ordinary SSH, transfers
and local remote-session support are independent of hosted cloud.

The [PR #1 requirements](../../01-build-zmux/references/pr-1-parity.md) remain required:
owned config/origins, Better Auth organizations and S256 PKCE, session/member/client
checks, MFA/localized mail, protected PostgreSQL migrations, separate platform identities,
session-bound relay tickets, readiness, protected distribution, no legacy raw-token paths,
admin/transaction invariants and correct release metadata. H01/H02/X02/D01 own these
jointly; V01 verifies each original matrix row rather than inheriting cmux's test claims.
