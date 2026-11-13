# Hosted acceptance: `9f2f13c`

**Source:** `9f2f13c3f6742ef8b69af10f6a75a9960ef88682`
**Scope:** Observe the existing hosted Foundation and universal development runs, then verify failure diagnostics. No workflow dispatch, retry, cancellation, source/index/status/plan edit, local test, app launch, commit, or push.

## Foundation

Foundation run [43386617224](https://github.com/ashutoshpw/zmux/actions/runs/43386617224) completed successfully at the requested source. Counts from the completed job logs:

- Ubuntu: **763 passed, 0 failed, 7 ignored** across 95 Rust test-result summaries.
- macOS: **761 passed, 0 failed, 7 ignored** across 95 Rust test-result summaries.

The separate baseline `terminal-selection`, `profile-operations`, `sidebar`, and `completed-history` jobs were skipped. The development job's own terminal-selection and browser-link acceptance step did run and pass.

## Universal development run

Universal run [43386617074](https://github.com/ashutoshpw/zmux/actions/runs/43386617074) checked out the exact requested SHA. Swift type-checking and both Linux scheduler jobs passed. In development job [131757457189](https://github.com/ashutoshpw/zmux/actions/runs/43386617074/job/131757457189), **Validate Rust core and foreign ABI**, **Build universal native development app**, **Retain testable candidate before native acceptance**, and **Exercise terminal selection and browser links in the universal app** all passed.

The first primary failure was **Exercise packaged app and native rendering**. The native smoke receipt at `native-smoke.json.stages.json` records `state=failed`, `currentStage=browser_automation`, and:

```text
BridgeFailure(message: "Expanded workspace group drop points are outside the visible sidebar viewport")
```

The complete failed job log reports the same native smoke error and exits with code 1. This is a packaged-app native smoke failure, not a workflow timeout. The native sidebar and status-menu step, final development-build upload, and build receipt were skipped. The Programs acceptance and `zmux-sidebar-programs.png` were not produced. A candidate ZIP was retained, but it is not the required final artifact. No `zmux-macos-dev-universal-9f2f13c3f6742ef8b69af10f6a75a9960ef88682` artifact was uploaded or accepted; package metadata, architectures, signing, and notarization were not verified.

## Failure evidence

The full job log is `/tmp/zmux-osc7501-final-9f2f13c/universal-failed-job-full.log` (**359,620 bytes**, SHA-256 `33c8a01e1d0a6d5b621a661672576b1f0ba1dba9677182286fef038214283751`). The uploaded [native diagnostics artifact](https://github.com/ashutoshpw/zmux/actions/runs/43386617074/artifacts/14020855992) is **30,209,673 bytes** and matches its API SHA-256 `abd6f646c6c0857d2f67bed37d4fd325656869f926847bfd6702f3d791f0a57e`. ZIP CRC and bounded safe extraction passed: 445 entries and 37,655,385 expanded bytes. The stage receipt and extracted diagnostics are under `/tmp/zmux-osc7501-final-9f2f13c/native-diagnostics-extracted/`.

The candidate artifact listed by the run is `zmux-macos-candidate-universal-9f2f13c3f6742ef8b69af10f6a75a9960ef88682` (26,770,944 bytes; API SHA-256 `ad095f1e361e2f1c54a98b3471fe288d28b3e9b8de013d074ee0c0f50a7382f1`). It remains candidate-only.

## Reused Ghostty linkage and local evidence

`/tmp/zmux-osc7501-final-9f2f13c/retained-linkage-reuse-verification.json` confirms all 229 retained linkage source pins match this source. The accepted retained run is `43371114352`, from `2e2c9d3445574b51569a2428bf8962061332165d`; no fresh engine build or linkage claim was made. Its retained evidence covers both Apple-target compile/link phases and ARM execution of engine 4/4, linked Program Status 1/1, Session 4/4, and linked real-PTY Program Status 1/1. Intel runtime and native-host/AppKit acceptance are not claimed.

The parent-supplied local `scripts/check` result remains **763/0/7 in 261.59 seconds** and was not rerun. The checkout remained at the requested SHA. Existing dirty paths were preserved; nothing was staged or committed.

## Parent scheduled-message evidence completion

The extracted `native-schedule-messages-smoke.json` is present at this exact
source and records `state=passed`, all 16 checks, including lost-create-reply
receipt recovery without duplicate create, edits, cancellation and CAS rejection.
The bounded RPC summary is available, 69 records, no malformed records and no
order truncation. Its prefix is status/targets/list/history/create/receipt_status/
show, with one create before the first receipt query and observed after-drop
receipt status. This smoke executes before the later broad browser-automation
workspace-drag failure. The earlier receipt failure did not recur; diagnostic
changes alone do not establish its historical cause or a functional repair.
