# Hosted acceptance: `2efb76d`

**Source:** `2efb76d9bcbb9e0a00015602b8bcaf81c00920de` on `main`
**Scope:** Finish and inspect only the three named existing hosted runs. No workflow dispatch, retry, cancellation, local build or test, source/status/plan/index edit, app launch, commit, or push. The new task reference and diagnostics below are the only outputs.

## Foundation checks

Foundation run [43407437043](https://github.com/ashutoshpw/zmux/actions/runs/43407437043) completed successfully at the requested source. Counts parsed from both completed job logs:

- macOS: **761 passed, 0 failed, 7 ignored**, across 95 Rust test-result summaries.
- Ubuntu: **763 passed, 0 failed, 7 ignored**, across 95 Rust test-result summaries.

The baseline `profile-operations`, `completed-history`, `terminal-selection`, and `sidebar` jobs were skipped. The universal workflow's development job has separate integrated coverage, described below.

## Universal macOS development run: first primary failure

Universal run [43407437131](https://github.com/ashutoshpw/zmux/actions/runs/43407437131) reached terminal `failure` at the exact requested source. Swift type-checking and both Linux scheduler jobs passed. The first primary failure was development job **131826686970**, step **Validate Rust core and foreign ABI** (`scripts/check`), where `cargo test -p zmux --test ssh` reported **1 passed, 2 failed**:

- `managed_ssh_opencode_tracks_root_events_refreshes_long_turns_and_preserves_config`
- `managed_ssh_codex_uses_root_thread_events_and_ignores_stop_hook_decisions`

The one passing test was `ssh_cli_opens_an_actual_remote_session_and_reopen_never_replays_its_command`. Both failures panicked at `apps/cli/tests/ssh.rs:166:13` after the remote terminal did not reach its expected state. The log records `EPERM: Operation not permitted` and `managed SSH lifecycle stream closed (hello=false, active=false)`. In the Codex fixture's captured terminal output, the generated helper text is visibly garbled and Python reports `File "<stdin>", line 42: SyntaxError: '(' was never closed`. The OpenCode fixture displays the intended single untracked fallback message after provider-route admission was not confirmed. These are observed failure details; they do not establish the historical admission cause or prove a functional repair.

The complete failed job log is `/tmp/zmux-osc7501-final-2efb76d/universal-failed-job-131826686970-full.log` (**120,661 bytes**, SHA-256 `6c4c20e16d62c28c0a1f4e1a12d2de7a922edcea582550a98f847019bb86deea`). The failed job's diagnostics-upload step ran, but its log says **“No files were found … No artifacts will be uploaded.”** A direct run-artifact API listing returned only the three scheduler input artifacts and no `Zmux-native-diagnostics-2efb76d9bcbb9e0a00015602b8bcaf81c00920de` artifact. Since no diagnostic ZIP exists, there is no diagnostic-artifact API digest, size, CRC, or extraction result to report. The API inventory is saved at `/tmp/zmux-osc7501-final-2efb76d/universal-run-artifacts-api.json`.

This failure stopped the universal app build, candidate retention, terminal-selection/browser smoke, packaged-app/native smoke, native sidebar/status-menu acceptance, downloadable build upload, and build receipt. No final or candidate app artifact, native acceptance JSON, or sidebar screenshot was produced. The successful scheduler artifacts are build inputs and do not substitute for the missing failure diagnostics or final app.

## Retained Ghostty linkage

Retained linkage run [43407437492](https://github.com/ashutoshpw/zmux/actions/runs/43407437492) completed successfully at `2efb76d9bcbb9e0a00015602b8bcaf81c00920de`. Its retained artifact is [terminal-engine-linkage-2efb76d9bcbb9e0a00015602b8bcaf81c00920de](https://github.com/ashutoshpw/zmux/actions/runs/43407437492/artifacts/14027538458), ID `14027538458`, **22,998,332 bytes**, API SHA-256 `33e57a5fc6fe0a04dda8ce2531e157ff83f52e2b6f2336e197ee4505b7c40ae7`. The downloaded archive matches the API size and digest; ZIP CRC and bounded safe extraction passed (**368 entries**, **70,707,368 expanded bytes**). Extracted files are under `/tmp/zmux-osc7501-final-2efb76d/ghostty-linkage-extracted/`.

The run used the accepted `ghostty-7501-v1` linkage, included Session runtime, and did not link the native host. Both exact target acceptance manifests passed. The current linkage receipt contains **229** source pins. All 229 were resolved as constrained regular Git blobs at the exact source SHA and their content SHA-256 values matched. Normalization yields **228 unique repository paths** and one expected alias: `tools/terminal-feasibility/../../scripts/process_group.py` resolves to `scripts/process_group.py`. The current-source `source.json` and `linkage.json` manifests are also pinned and bound to `ghostty-7501-v1` (SHA-256 `d19f74d9138b065014b17fb9deef9d9a2e653e6b0fd8c022eef3d1f7868d9cd8` and `7ddc18b2c12a524358378e46326868c1d83ed88c683bf0f622d6485feaf2e077`). Both accepted library hashes match the exact-source linkage manifest and the previously accepted library bytes; no engine producer was rerun or engine rebuilt.

Both Apple targets passed all eight compile/link phases: engine, linked Program Status, Session, and linked Session Program Status for arm64 and x86_64. ARM64 execution passed engine **4/4**, linked Program Status **1/1**, Session **4/4**, and linked real-PTY Program Status **1/1**. Intel execution, native-host/AppKit linkage, minimum-OS runtime execution, and engine rebuild are not claimed.

Structured linkage verification is in `/tmp/zmux-osc7501-final-2efb76d/linkage-final-verification.json`, `/tmp/zmux-osc7501-final-2efb76d/linkage-source-pins-verification.json`, and the two `/tmp/zmux-osc7501-final-2efb76d/linkage-accept-*.json` files. The outer archive digest/CRC/extraction summary is `/tmp/zmux-osc7501-final-2efb76d/ghostty-artifact-download.json`.

## Preserved scope and checkout state

- No local `scripts/check`, Cargo test, build, or app launch was run. Local work was limited to downloading and verifying the retained linkage artifact and capturing hosted evidence.
- The pre-existing dirty paths were preserved: `.agents/tasks/2026-10-18/01-cloud-origin-build/status.md`, `.agents/tasks/2026-11-12/01-osc-7501-support/status.md`, `.github/workflows/profile-lifecycle-probe.yml`, and `tools/native-profile-lifecycle/`.
- Nothing was staged or committed. The universal failure is the current stopping point; it is not native app acceptance or a downloadable build.
