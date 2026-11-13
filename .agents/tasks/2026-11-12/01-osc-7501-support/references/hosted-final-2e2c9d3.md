# Hosted acceptance: `2e2c9d3`

**Source:** `2e2c9d3445574b51569a2428bf8962061332165d`
**Scope:** Observe only the three existing hosted runs and verify retained artifacts. No dispatch, retry, cancellation, local build/test, source/index/status/plan edit, commit, push, or app launch.

## First primary failure

Foundation run [43371105849](https://github.com/ashutoshpw/zmux/actions/runs/43371105849) completed successfully at the requested source. The Ubuntu job reported **763 passed, 0 failed, 7 ignored** across 95 Rust test-result summaries. The macOS job reported **761 passed, 0 failed, 7 ignored** across 95 summaries.

Universal macOS run [43371105829](https://github.com/ashutoshpw/zmux/actions/runs/43371105829) checked out the same SHA. Swift type-checking, both Linux scheduler jobs, Rust validation, universal app build, and candidate retention passed. The first primary failure was **Exercise terminal selection and browser links in the universal app** (job `131707842944`).

The failed step ran:

```text
python3 -B tools/native-terminal-selection/run.py --skip-build --output dist/native-terminal-selection-universal
```

Its receipt reports `RuntimeError: metadata-compile failed: exit=1, timed_out=False`. `metadata-compile.log` shows `swiftc` failed because `NotificationDelivery.swift` references two names that were not in the compile inputs:

```text
NotificationDelivery.swift:326:41: error: cannot find 'ProgramAlertEventGate' in scope
NotificationDelivery.swift:393:35: error: cannot find type 'ProgramAlertSnapshotMode' in scope
```

Both declarations are in `apps/macos/Sources/ProgramAlertDelivery.swift`; that file is absent from the recorded `metadata-compile` command. The universal app build itself had passed before this acceptance helper failed.

This was not the intentional mutation-child EOF case: the complete failed job log contains no EOF/child EOF diagnostic. It also does not show a poisoned-lock cascade. Its only `poison` match is the passing test `pull_request_wire_owner_rejects_noncanonical_records_without_poisoning_snapshot ... ok` from the earlier successful Rust validation.

The complete failed job log is retained at `/tmp/zmux-osc7501-final-2e2c9d3/universal-failed-job-131707842944-full.log` (**210,785 bytes**, SHA-256 `3eec5081663d0a6a641a9c3523662a809d231768b15b4448cf427ca84388afe6`). The workflow's diagnostic artifact, ID `14013114063`, was downloaded and verified against the API digest: **30,104,736 bytes**, SHA-256 `ca47c94cc32fe59166faeb466325e4e3dc261bbeae615774dce9b798bdecf8f0`; ZIP CRC and safe extraction passed (411 entries, 37,292,081 expanded bytes). The exact receipt and compiler log are under `/tmp/zmux-osc7501-final-2e2c9d3/universal-diagnostics-artifact/extracted/native-terminal-selection-universal/`.

The workflow retained candidate artifact `zmux-macos-candidate-universal-2e2c9d3445574b51569a2428bf8962061332165d` (ID `14012714953`, 26,762,846 bytes, ZIP SHA-256 `980de1fbf13efffaa0dd183ff8328676a53491682733e642e52e34d6aa7361d3`). It is a candidate only. The packaged-app and native-sidebar acceptance steps did not run, and the required `zmux-macos-dev-universal-2e2c9d3445574b51569a2428bf8962061332165d` final artifact was not uploaded. No final app checksum, packaging/signing verification, native Programs acceptance, or screenshot inspection of `zmux-sidebar-programs.png` is claimed.

## Retained Darwin linkage

Retained linkage run [43371114352](https://github.com/ashutoshpw/zmux/actions/runs/43371114352) completed successfully at the requested source with `candidate_set=ghostty-7501-v1`, `INCLUDE_SESSION_RUNTIME=true`, and `INCLUDE_NATIVE_HOST=false`.

Artifact [terminal-engine-linkage-2e2c9d3445574b51569a2428bf8962061332165d](https://github.com/ashutoshpw/zmux/actions/runs/43371114352/artifacts/14012631713), ID `14012631713`, is **22,998,343 bytes**. The downloaded archive SHA-256 `617d63591b84b14a3828dfb4712115ab8da81fe9953c6827eed74aaea46e53c4` matches the GitHub API digest. ZIP CRC passed; bounded safe extraction passed for 368 entries and 70,707,367 expanded bytes.

All **229** receipt source pins match `git show` at the exact source SHA after normalized in-repository path checks (228 unique normalized paths, one alias). Compared with the prior verified `82df335` linkage receipt, only `crates/session-runtime/tests/shell_integration.rs` changed; ARM64 and x86_64 engine library hashes are identical. The versioned candidate manifests and independent review bind the accepted libraries to producer run `43315098805`, artifact `13988885464`, and archive SHA-256 `aa232650b06d2c9184e94cfd41f38d80d91f61901726a703aed25b7ade69b855` (9,322,902 bytes).

All eight engine, Program Status, Session, and real-PTY Program Status compile/link phases passed for both Apple targets. ARM64 execution passed: engine **4/4**, linked 7501 Program Status **1/1**, Session **4/4**, and linked real-PTY 7501 Session **1/1**. The receipt says no engine rebuild, no AppKit/native-host link, no Intel execution, and no minimum-OS runtime execution.

Verification records:

- `/tmp/zmux-osc7501-final-2e2c9d3/foundation-summary.json`
- `/tmp/zmux-osc7501-final-2e2c9d3/linkage-final-verification.json`
- `/tmp/zmux-osc7501-final-2e2c9d3/linkage-source-pins-verification.json`
- `/tmp/zmux-osc7501-final-2e2c9d3/linkage-run-inputs.txt`
- `/tmp/zmux-osc7501-final-2e2c9d3/universal-failure-summary.json`

The checkout was at the requested SHA before and after these observations. Pre-existing dirty files were preserved; only this reference file was added. Nothing was staged or committed.

The parent-supplied local `scripts/check` result remains **763/0/7 in 267.16 seconds**; it was not rerun. The preserved pre-existing dirty paths were `.agents/tasks/2026-10-18/01-cloud-origin-build/status.md`, `.agents/tasks/2026-11-12/01-osc-7501-support/status.md`, `.github/workflows/profile-lifecycle-probe.yml`, and `tools/native-profile-lifecycle/`.
