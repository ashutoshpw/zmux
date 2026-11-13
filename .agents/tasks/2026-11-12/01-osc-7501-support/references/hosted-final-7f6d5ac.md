# Fail-fast hosted validation: `7f6d5ac`

**Source:** `7f6d5ac29c4d51d00d4f08f416d2dae7b1d41557` on `main`
**Scope:** Continue only the two existing hosted runs. No dispatch, retry, source change, local check rerun, Ghostty producer/linkage run, app launch, commit, or push.

## Terminal outcomes

### Foundation checks passed

Run [43429978647](https://github.com/ashutoshpw/zmux/actions/runs/43429978647) completed successfully at the exact source SHA.

- Ubuntu job `131900749673`: **765 passed, 0 failed, 7 ignored**, from 95 Rust test-result summaries. Full job log: `/tmp/zmux-osc7501-final-7f6d5ac/foundation-ubuntu-job-131900749673-full.log`, 300,901 bytes, SHA-256 `ff02c7a7da43322c87d44b222517ec9887b1ee6f3011d9e062ad003bd7cdffa0`.
- macOS job `131900749827`: **763 passed, 0 failed, 7 ignored**, from 95 Rust test-result summaries. Full job log: `/tmp/zmux-osc7501-final-7f6d5ac/foundation-macos-job-131900749827-full.log`, 300,450 bytes, SHA-256 `c3edc6430b7ceab48cea581119a271724c8764f82afa51e3ce06679090a71cde`.
- Parsed totals and hashes are retained in `/tmp/zmux-osc7501-final-7f6d5ac/foundation-test-counts.json`.

### macOS development run failed at packaged Cloud terminal lifecycle

Run [43429978535](https://github.com/ashutoshpw/zmux/actions/runs/43429978535) reached terminal `failure` at the exact source SHA. Development job `131902215932` completed at 2026-11-13T13:32:48Z.

Before the failure, both Linux scheduler helper jobs, Swift typecheck and its scheduler queue-layout check, Rust/foreign-ABI validation, the universal build, candidate retention, and terminal-selection/browser-link acceptance passed. The development Rust check produced **763 passed, 0 failed, 7 ignored** across 95 summaries. The terminal-selection receipt is complete at the exact SHA.

The pre-smoke native window receipt `/tmp/zmux-osc7501-final-7f6d5ac/native-diagnostics-extracted/native-windows.json` also reports `success=true` on the exact SHA. Its observed fields include `prepared.workspaceMove=true`, `reopened.success=true`, `reopened.workspace_todos_restored=true`, and `reopened.workspace_split_ratio_restored=true`.

The first primary failure was step 22, **“Exercise packaged app and native rendering”** (`scripts/check-macos-app --skip-accessibility-save-tests`). Its Cloud terminal lifecycle smoke did not publish `native-cloud-terminal-lifecycle.json`. The exact reported errors were:

```text
The packaged app did not produce Cloud terminal lifecycle evidence
Native Cloud terminal lifecycle failed: Native sharedTerminalOperation(request:grant:):buffer_copy token=12 failed (-4)
```

The lifecycle stderr log is 120 bytes and contains the second line; stdout is empty. The failure-process receipt says `scope=read_only_after_failed_smoke` and `second_app_launched=false`. No cause is inferred from this error.

This failure occurred before the broad packaged native smoke. `native-smoke.json` and `native-smoke.json.stages.json` are absent, so `FilePanelSmoke` did not run. This run therefore provides no per-tab status for the fixed sixteen indexes and no `FileLoadGate` counts; it does not establish that the earlier file-panel timeout is fixed. The later native sidebar/Programs step and downloadable-build upload were not reached. `native-sidebar.json`, `native-sidebar-programs.json`, and `zmux-sidebar-programs.png` are absent.

The complete failed development-job log is `/tmp/zmux-osc7501-final-7f6d5ac/universal-development-job-131902215932-full.log`, **338,278 bytes**, SHA-256 `f0a4a1d2a9cc43b7b7461edf91d48358006dbf9eb3452b83ad6f326926537aa1`. It includes the failure and completed diagnostic upload.

## Failure artifact and candidate distinction

The diagnostic artifact is [Zmux-native-diagnostics-7f6d5ac29c4d51d00d4f08f416d2dae7b1d41557](https://github.com/ashutoshpw/zmux/actions/runs/43429978535/artifacts/14042197034), ID `14042197034`. GitHub API size is **30,150,405 bytes** and API SHA-256 is `d9a833a6b37d9f1f8b103aed6e50f5eab25056edadfdb5e8d0b2777a6163bb31`. The downloaded archive matched both. ZIP CRC passed; safe bounded extraction passed with **435 entries** and **37,480,831 expanded bytes**. Verification receipt: `/tmp/zmux-osc7501-final-7f6d5ac/native-diagnostics-14042197034-verification.json`; extracted diagnostics: `/tmp/zmux-osc7501-final-7f6d5ac/native-diagnostics-extracted/`.

The run contains candidate artifact `zmux-macos-candidate-universal-7f6d5ac29c4d51d00d4f08f416d2dae7b1d41557`, ID `14041926737` (API size **26,776,702 bytes**, digest `sha256:baf154cb4d4d9ec1440d5199ebb60859c567eda92545620a5299748eaf63dcc1`). It is only a candidate and was not verified or accepted. The exact final artifact `zmux-macos-dev-universal-7f6d5ac29c4d51d00d4f08f416d2dae7b1d41557` is absent. No accepted app download, sidebar Programs report, or Programs screenshot exists for this run.

## Preserved validation scope

- The parent-provided local aggregate remains **765 passed, 0 failed, 7 ignored in 263.59 seconds**; it was not rerun here.
- Ghostty linkage remains retained from run `43407437492`, artifact `14027538458`, accepted at source `2efb76d9bcbb9e0a00015602b8bcaf81c00920de`. The parent’s `/tmp/zmux-osc7501-final-7f6d5ac/retained-linkage-reuse-verification.json` verifies all **229/229** receipt Git blobs against this source. This is retained-source reuse, not a `7f6d5ac` engine rebuild or producer/linkage run. Intel execution and native-host/AppKit linkage are not claimed.
- No Intel runtime, physical-Mac, or production acceptance is claimed. The user’s existing dirty task/status and profile-lifecycle paths remain untouched; `HEAD` remains the requested SHA.
