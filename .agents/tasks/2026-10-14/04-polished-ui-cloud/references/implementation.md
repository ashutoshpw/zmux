# Contracts and evidence

## Ownership
- Native UI worker: App.swift, Models.swift, WorkspaceViews/UIComponents and visual terminal styling.
- CLI worker: CLI, desktop runtime/control listener, native bridge and Rust dependencies.
- Cloud worker: services/control-plane, cloud validation workflow and check script.
- Parent: original icon, terminal title metadata, native cloud client, packaging, deployment workflow and integration.

## Native cloud contract
Origin is an explicit owned HTTPS service (loopback HTTP permitted for development).
Client `zmux-macos-dev`; callback `zmux-dev://oauth/callback`. Browser authorization uses
state and S256 PKCE at `/v1/native/authorize`, code exchange `/v1/native/token`.
Authenticated `/v1/me`, `/v1/devices`, heartbeat, revoke use bearer headers.
No credentials in callback URLs. Native token storage uses macOS Keychain.

## Test owners
- CLI executable plus real desktop socket owns help, shared action behavior and transport bounds.
- Cloud service with real disposable PostgreSQL owns authorization, code exchange and cross-user access.
- Native packaged smoke owns Swift/C linkage, title propagation, icon/bundled CLI and UI construction.
- Existing Rust tests remain owners of workspace/session/parser invariants; avoid redundant fixtures.

## Portable and cloud validation
- Parent `scripts/check`: 27 Rust tests, strict Clippy, formatting, license inventory and real C ABI pass.
- Parent `scripts/check-cloud` with fresh disposable PostgreSQL: migrations, TypeScript, 11 integration tests / 74 assertions, production Docker image pass.
- Initial hosted runs exposed snapshot-read contention, macOS fixture socket length and a macOS-only Clippy warning. Fixed with bounded snapshot reads, compact test paths and platform expression cleanup.
- C ABI fixture now pins its own shell to `/bin/sh`; user zsh startup consumed early input. SIGPIPE and exited-session assertions remain enabled.
- Hosted cloud checks: https://github.com/ashutoshpw/zmux/actions/runs/38636444105 (success).
- Cloud package: https://github.com/ashutoshpw/zmux/actions/runs/38636451380/artifacts/11928271897 (source `870c617f37916e1809bd6aa220d9ba8567823eb3`). Parent downloaded and verified gzip image checksum, Docker manifest source tag and receipt; `deployed: false`.
- No production provider selected. Remote PTY, VM, relay, mail, MFA and billing parity remain separate roadmap work.

## Independent review
- Fresh read-only reviewer `/root/review_ui_cloud` reviewed `f49a860..d6b0014` and returned `ASTRA REVIEW / VERDICT: ship`, no actionable findings.
- Requested reviewer model/effort: gpt-5.6-sol/high; realized runtime metadata unobservable.
- Remaining operations: cloud record retention cleanup, automatic native device heartbeat; native compile/artifact gates were pending during review.
- Hosted Swift compile then caught an AppKit inherited `owner` property collision; renamed the weak app controller field in `5d30962`.

## Native delivery receipt
- Native run https://github.com/ashutoshpw/zmux/actions/runs/38637020769 succeeded for `5d309621da0f0eb53521db3135b234476ef5c321`.
- Artifact https://github.com/ashutoshpw/zmux/actions/runs/38637020769/artifacts/11929200946 expires 2026-10-28.
- ZIP SHA-256 `1e3a8ed8c3a40009c6fb5ecbafa7c5dda07526790ca654daaadfea92ca8b2fff`.
- Parent downloaded and verified archive integrity, arm64/x86_64 Mach-O headers, executable permissions, icon, notices and matching source metadata. Cached native PNG visually inspected.
- All native smoke fields pass, including actual OSC2 title literal, accessible controls, bundled CLI control, callback contract and prior local-terminal behavior.
- WindowServer capture returned 1; cached AppKit image available. Manual native IME/accessibility, live deployed sign-in, Intel execution and measured resource budgets remain gates.

## Final review and foundation follow-up
- Reviewer `/root/review_delivery` independently verified the AppKit rename and downloaded artifact receipts, returning `ship` with no findings.
- Separate macOS foundation lane exposed an unauthorized-peer fixture race: it wrote after the server intentionally rejected the peer before reading. Test-only `3585949` now reads and asserts immediate `unauthorized` without sending a request. Production is unchanged.
- Fresh follow-up review returned `ship`; parent `scripts/check` passes and hosted foundation run https://github.com/ashutoshpw/zmux/actions/runs/38637673711 passes Linux and macOS.
- API-equivalent cost receipt unavailable: parent and delegated token usage are not exposed. No cost or savings estimate is claimed.

## Final artifact
- Latest native run38637673581 passed at `3585949a60845fbdcf4d76b3d49b2f62f970a104`; artifact11928138411 downloaded and independently verified with the same archive, architecture, icon, permissions, notices and smoke checks.
- Final ZIP SHA-256 `b68e41657516c5f0166978d88ce1be16bff107c77d5da5d5dc506bae7b322d1c`. See development-build docs for the download link.
