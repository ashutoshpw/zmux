# Hosted outcomes for `848e72c`

**Source:** `848e72c75d5c06eefaba7c95b08c5c24b4318c32` on `main`
**Scope:** Continue the two existing hosted runs only. No dispatch, retry, source change, local check/build, app operation, linkage rerun, commit, or push.

## Terminal outcomes

### Foundation passed

[Foundation checks run 43441181807](https://github.com/ashutoshpw/zmux/actions/runs/43441181807) completed successfully on the exact source SHA.

- Ubuntu job `131938873131`: **767 passed, 0 failed, 7 ignored** across 95 Rust test-result summaries; the Linux-only browser DOM gate passed **148 cases**. Full log: `/tmp/zmux-osc7501-final-848e72c/foundation-ubuntu-job-131938873131-full.log`, 301,324 bytes, SHA-256 `38f33c0867b19226417b31105fb3f8d45ff28836c78029c4ad63eaf2934ba8b5`.
- macOS job `131938873471`: **765 passed, 0 failed, 7 ignored** across 95 Rust test-result summaries. The Linux-only browser DOM step was skipped. Full log: `/tmp/zmux-osc7501-final-848e72c/foundation-macos-job-131938873471-full.log`, 300,871 bytes, SHA-256 `7d07b3abee0143745de3d7384f16f885e9961dbf9aba9bfcea487e2a5738a91c`.
- Run and job API responses are retained at `/tmp/zmux-osc7501-final-848e72c/foundation-run-api.json` and `foundation-jobs-api.json`.

### Universal macOS development run failed at sidebar acceptance

[macOS development run 43441181931](https://github.com/ashutoshpw/zmux/actions/runs/43441181931) completed with conclusion **failure** on the exact source SHA. Swift type-check and scheduler queue-layout checks passed; both Linux scheduler jobs passed; native prechecks and Rust/foreign-ABI validation passed. The hosted macOS `scripts/check` produced **765 passed, 0 failed, 7 ignored** across 95 test-result summaries. Universal app build, candidate retention, and terminal-selection/browser-link acceptance also passed.

Packaged app/native rendering passed before the failure. `native-smoke.json` reports `source_sha=848e72c75d5c06eefaba7c95b08c5c24b4318c32`, `success=true`, and `file_panel=true`; its stages receipt says `assertions_complete` and includes `file_panel`. The Cloud terminal lifecycle receipt reports `state=passed`. These results do not identify the cause of earlier failures.

The first primary failure was job `131940000318`, step 23, **“Exercise native sidebar and system status menu”**, running `scripts/check-native-sidebar --skip-build`. The failure was logged at `2026-11-13T14:52:44Z`. The script raised:

```text
RuntimeError: Native sidebar/status assertions did not publish a success receipt; inspect native-sidebar.stderr.log
```

The underlying `native-sidebar.stderr.log` message is:

```text
Native sidebar smoke failed: BridgeFailure(message: "Populated Agents rows or grouped state headers are incomplete")
```

The same stderr records a workspace-group disclosure action and header mouse-down before that assertion. No cause is inferred. `native-sidebar-process.json` records the same source SHA, `success=false`, completed physical cleanup, and an absent app PID after cleanup.

After the failure, the workflow uploaded diagnostics successfully. Final downloadable-build upload was skipped. The run has no `zmux-macos-dev-universal-848e72c75d5c06eefaba7c95b08c5c24b4318c32` artifact. A **candidate only** exists as artifact `14047549086`, `zmux-macos-candidate-universal-848e72c75d5c06eefaba7c95b08c5c24b4318c32` (API size 26,782,978 bytes, digest `sha256:a140b564b9664d58261578bf8f3c42300632d9d512d5b0c15a12cea754a69869`). It is not an accepted build or delivery.

`native-sidebar.json`, `native-sidebar-programs.json`, and `zmux-sidebar-programs.png` were not produced. The diagnostic artifact includes `zmux-sidebar-failure.png` at `/tmp/zmux-osc7501-final-848e72c/native-diagnostics-extracted/zmux-sidebar-failure.png`; it is failure evidence, not a Programs acceptance screenshot.

## Failure evidence

- Complete failed development-job log: `/tmp/zmux-osc7501-final-848e72c/universal-development-job-131940000318-full.log`, 349,849 bytes, SHA-256 `6945e9e0413e245aa9c42ea8ab025c873022e798e53fd926cb28f4edc1755347`. It was fetched again after the job reached terminal state; the earlier failure-time fetch had the same size and hash.
- Diagnostic artifact: [ID 14048725676](https://github.com/ashutoshpw/zmux/actions/runs/43441181931/artifacts/14048725676), `Zmux-native-diagnostics-848e72c75d5c06eefaba7c95b08c5c24b4318c32`. GitHub API size is **30,556,284 bytes**, API digest `sha256:ddae9d94585e338ca0ddecb315d4114b2a9aca885a59e24c271c7afc4eb1ef0a`; downloaded size and SHA-256 match. ZIP CRC passed. Bounded, path-safe extraction passed for **458 entries / 38,158,541 expanded bytes**.
- API metadata, artifact inventory, and verification receipt: `/tmp/zmux-osc7501-final-848e72c/native-diagnostics-14048725676-api.json`, `failure-artifacts-terminal-api.json`, and `native-diagnostics-14048725676-verification.json`. The extracted diagnostics are under `/tmp/zmux-osc7501-final-848e72c/native-diagnostics-extracted/`.
- The native file-load gate passed in the hosted job: sixteen simultaneous loads stayed within three in flight and started in order; busy-like failures used 250/500/1000 ms back-off and failed explicitly afterward; other failures and unknown outcomes were not retried. The packaged native smoke separately reports `file_panel=true`. This execution did not reproduce the earlier file-panel timeout. The available passing receipts do not enumerate per-tab status for all 16 indexes, so that detail remains unverified here; the underlying earlier cause remains unknown.

## Retained Ghostty linkage scope

No linkage producer or linkage workflow was rerun. The existing `/tmp/zmux-osc7501-final-848e72c/retained-linkage-reuse-verification.json` records **229/229** retained Git blob inputs matching this source SHA, reusing run `43407437492` / artifact `14027538458`, whose source was `2efb76d9bcbb9e0a00015602b8bcaf81c00920de`. The original source-manifest receipt remains `/tmp/zmux-osc7501-final-2efb76d/ghostty-linkage-extracted/receipt.json`. This is retained-source verification, not a producer rerun. Intel runtime behavior and native-host/AppKit linkage or execution are not claimed.

## Preservation and limits

`HEAD` remains the requested SHA. No source, plan, task status, index, or unrelated work was edited. The pre-existing dirty task-status files, `.github/workflows/profile-lifecycle-probe.yml`, and `tools/native-profile-lifecycle/` were preserved. No final app artifact, native sidebar Programs receipt/image, Intel runtime, physical-Mac, notarization, production, or live-user acceptance is established by these runs.
