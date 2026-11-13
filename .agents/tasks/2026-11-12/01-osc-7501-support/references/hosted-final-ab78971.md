# Hosted validation and artifact acceptance: ab78971

**Verdict:** Both requested hosted runs reached terminal state on the exact source commit. Foundation failed in its Ubuntu provider-hooks test, and the universal macOS run failed during the packaged native smoke. The required final universal app artifact was not uploaded, so there is no accepted app download.

## Source and reused evidence

- Commit: https://github.com/ashutoshpw/zmux/commit/ab789718c1cea30b8cf610f9839bb1b1b1b665ad on main.
- The parent-preserved local aggregate is 765 passed, 0 failed, 7 ignored in 277.41 seconds. It was not rerun here.
- Existing Ghostty linkage reuse remains attributed to source 2efb76d9bcbb9e0a00015602b8bcaf81c00920de, run 43407437492, artifact 14027538458. The retained verification receipt records all 229 captured Git inputs matching ab78971 and engine_rebuilt=false. No new engine or linkage build was run, and the older artifact is not relabeled as an ab78971 build.

## Foundation checks: terminal failure

- Run 43418903359: https://github.com/ashutoshpw/zmux/actions/runs/43418903359. Exact head SHA was verified as ab789718c1cea30b8cf610f9839bb1b1b1b665ad. Terminal conclusion: failure at 2026-11-13T11:49:41Z.
- foundation (macos-latest) succeeded. Parsed 95 Rust test-result records from the hosted log: 763 passed, 0 failed, 7 ignored; summed per-record durations were 317.09 seconds. This hosted count is separate from the parent's preserved local aggregate.
- foundation (ubuntu-latest) failed in workflow step “Check existing Rust modules and foreign C ABI.” The first failing test was automatic_local_opencode_tracks_exact_root_and_preserves_user_configuration at apps/cli/tests/provider_hooks.rs:2715:9. Its exact assertion context was “automatic OpenCode launch did not reach its plugin host: LOCAL_OPENCODE_SHELL_READY.” The preceding production message reports OpenCode session plugin setup failed at stage=run_standalone_help_preflight, elapsed_ms=2009, then launched once without the lifecycle adapter. The log does not establish why that standalone help preflight failed.
- The remaining 11 test failures were PoisonError cascades from apps/cli/tests/provider_hooks.rs:661:44. The suite summary was 3 passed, 12 failed, 2 ignored.
- Complete failed job log: /tmp/zmux-osc7501-final-ab78971/foundation-ubuntu-job-131863658740-full.log, 53,339 bytes, SHA-256 0559e9c450a6bba2640962f08aff360c8494c58d4a986994102d71c92f214e5c. The GitHub job API receipt is foundation-ubuntu-job-131863658740-api.json.
- The completed run's artifact API list was empty. No Foundation diagnostic artifact was available. The failed log contains no child_status, stderr_class, stdoutclosedbeforeHello, EPERM, or garbled-probe record; no SSH startup cause is inferred from this failure.

## Universal macOS development build: terminal failure

- Run 43418903541: https://github.com/ashutoshpw/zmux/actions/runs/43418903541. Exact head SHA was verified as ab789718c1cea30b8cf610f9839bb1b1b1b665ad. Terminal conclusion: failure at 2026-11-13T12:15:22Z.
- swift-typecheck, both Linux scheduler jobs, Rust/foreign-ABI validation, universal app build, and terminal-selection/browser-links acceptance passed.
- Step 22, “Exercise packaged app and native rendering,” failed. The extracted native-smoke.json.stages.json reports scope=native_smoke_assertions, currentStage=file_panel, state=failed, and the exact failure: BridgeFailure(message: “Native file panel: timed out waiting for sixteen simultaneous file tabs loaded”). The full job log records the same failure followed by exit code 1.
- The next step, “Exercise native sidebar and system status menu,” and “Upload downloadable development build” were skipped. “Upload diagnostics on failure” succeeded. No final zmux-macos-dev-universal-ab789718c1cea30b8cf610f9839bb1b1b1b665ad artifact exists in the terminal API artifact list.
- Complete failed development-job log: /tmp/zmux-osc7501-final-ab78971/universal-development-job-131865014573-full.log, 238,771 bytes, SHA-256 f8c8f1ceb7e494ff6997f0d02c8efe6cdbd3eb76164c5d143f3e5e1a2544a542.

### Diagnostic artifact and JSON acceptance

- Downloaded diagnostic artifact 14034383331: https://github.com/ashutoshpw/zmux/actions/runs/43418903541/artifacts/14034383331. API size 30,444,830 bytes matched the downloaded file; API and downloaded SHA-256 both equal 5146130dc9ebe50b4473b0d64ffd74804bdbea58a2dc34b98b8667f0151f7e47. ZIP CRC validation passed. Safe bounded extraction verified 450 entries and 38,054,792 expanded bytes under limits of 5,000 entries and 786,432,000 bytes. Artifact expires 2026-11-20T12:15:13Z. Receipts are saved under /tmp/zmux-osc7501-final-ab78971/.
- native-schedule-messages-smoke.json reports state=passed for all 16 named checks on the exact source SHA. All five captured mutation receipts were applied; the RPC receipt records 69 observations and the unknown-outcome UI was exercised. This is an actual smoke result, not an inference from the file's presence.
- native-terminal-selection-universal/native-selection.json reports state=complete on the exact source SHA; all 48 expected/observed cases match. The selection receipt records a reused existing universal package and matching source metadata. Its native-selection.log and metadata-compile.log are empty; metadata-contract.log and swift-source-check.log were inspected.
- native-windows.json reports success=true for the window close/reopen fixture. It does not establish the corrected window-minimum fixture; there is no window-minimum acceptance receipt in this run.
- The full native-smoke.json is absent; its .stages.json sidecar is the failure receipt above. native-sidebar-programs.json and native-sidebar.json are absent, and the exact expected screenshot path /tmp/zmux-osc7501-final-ab78971/universal-native-diagnostics-extracted/zmux-sidebar-programs.png is missing. Programs checks therefore did not run or pass.
- The native stderr log contains three managed-SSH startup-retry observations: “managed SSH lifecycle stdout closed before Hello (child_status=exit_code:255, stderr_class=ssh_connection_or_master_failure).” It separately contains six “remote lifecycle process cancellation: EPERM” lines. These observations are recorded separately from the file-panel timeout; no causal link is claimed. The log also has coordinator messages for browserEvent failure (-6), an unsaved file, and “could not create image from window”; the stage receipt still identifies the terminal native-smoke failure as file_panel.
- The hosted native runner was an ARM64 VirtualMac2,1 on macOS 15.8.2. No Intel runtime, physical-Mac, or user-machine acceptance is claimed.

### Images for inspection

- Main native smoke screenshot: /tmp/zmux-osc7501-final-ab78971/universal-native-diagnostics-extracted/Zmux-native-smoke.png. Visual inspection shows the app and browser smoke fixture; the Programs section has no reports.
- Passed schedule UI screenshot: /tmp/zmux-osc7501-final-ab78971/universal-native-diagnostics-extracted/zmux-scheduled-messages.png.
- Other retained UI screenshots: zmux-browser-smoke.png, zmux-workspace-drag.png, zmux-local-tmux-smoke.png, and zmux-ssh-reconnect.png in the same extraction directory.

## Final artifact status

- Candidate artifact 14034952209 exists with API metadata: 26,771,307 bytes, SHA-256 a4799a5c254b827fbd764841ab0c79ce76cadb50c8bb524af2aef5ad809337f8, expires 2026-11-27T12:09:48Z. It is explicitly a candidate and was not accepted or run through the final app verifier.
- The selection-test receipt describes its reused package as universal arm64/x86_64, minimum macOS 14.0, ad-hoc signed, not notarized. It records inner package name zmux-dev-universal-ab78971.zip and SHA-256 0845a04a76fae9b289791c12e4fee0ff0b2d3f507f8edd7aa5514e58e5c44c58. This receipt is candidate/test evidence, not final artifact verification.
- No final artifact URL, final outer or inner checksum, final packaged metadata, or final signing/notarization acceptance is available. /tmp/zmux-osc7501-verify-app.py was not run because the required final artifact was not uploaded.
