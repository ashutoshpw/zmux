# Hosted acceptance: `0742e26`

**Source:** `0742e26029a872449518a23eb5c5d9f30f7e3d0a`
**Scope:** Observe the existing Foundation and universal development runs. No workflow was dispatched, retried, or cancelled. No source, status, plan, index, app, or local validation was changed or run.

## Foundation

The separate Foundation run `43386617224` was reported successful at the preceding exact source: Ubuntu **763 passed, 0 failed, 7 ignored**; macOS **761 passed, 0 failed, 7 ignored**. Baseline browser locator checks were skipped as previously recorded. This does not make the universal development run below successful.

## Universal development run

Run [43395036940](https://github.com/ashutoshpw/zmux/actions/runs/43395036940) checked out the requested source and reached terminal **failure**. The development job [131785226948](https://github.com/ashutoshpw/zmux/actions/runs/43395036940/job/131785226948) failed in **Validate Rust core and foreign ABI** while running `scripts/check`.

The first primary failure was Rust integration test `generic_provider_observations_are_local_control_actions_with_explicit_ends` in `crates/desktop-runtime/tests/agent_lifecycle.rs:805:10`:

```text
called `Result::unwrap()` on an `Err` value: Busy
test result: FAILED. 2 passed; 1 failed; 0 ignored; 0 measured; 0 filtered out
error: test failed, to rerun pass `-p zmux-desktop-runtime --test agent_lifecycle`
Process completed with exit code 101.
```

The failure is an observed test failure, not a workflow timeout. The Rust/ABI step started at 08:45:21 UTC and failed at 08:51:03 UTC. The Linux x86_64 and aarch64 scheduler jobs, Swift type-check, scheduler queue-layout receipts, and the earlier native source/fixture checks completed successfully.

The subsequent universal app build, candidate retention, terminal-selection/browser-link smoke, packaged app/native rendering smoke, native sidebar/status-menu smoke, and downloadable build upload were skipped. Therefore no final universal build, build receipt, packaged Programs acceptance, schedule smoke receipt, or app-native stage receipt exists for this run. In particular, the 16-check scheduled-message smoke and broad workspace drag smoke did not run here; the outer current stage is not being used to infer their results.

## Failure diagnostics

The complete failed development-job log is `/tmp/zmux-osc7501-final-0742e26/universal-failed-job-full.log` (**144,167 bytes**, SHA-256 `17835c77bf3209f9e6e0dd8604946a800e4b2e8cf9a4eb1457367cc14f7f7589`). It includes the `Busy` panic, test result, exit code, skipped-step outcomes, and the diagnostics upload attempt.

The failure upload step `Upload diagnostics on failure` ran, but its configured `dist/` paths were absent. The job log says `No files were found with the provided path` and ends with `No artifacts will be uploaded.` The final run Artifacts API snapshot is `/tmp/zmux-osc7501-final-0742e26/run-artifacts-api.json`; it contains only the three Linux scheduler/queue-layout artifacts, with no `Zmux-native-diagnostics-0742e26029a872449518a23eb5c5d9f30f7e3d0a` archive. Since no relevant diagnostic archive was published, there was no diagnostic ZIP available to download, checksum, CRC-check, or safely extract. No unrelated scheduler artifact was treated as a diagnostic for this Rust failure.

## Delivery boundary

No `zmux-macos-dev-universal-0742e26029a872449518a23eb5c5d9f30f7e3d0a` final artifact was produced. Artifact metadata, packaged source/build metadata, universal architectures, macOS minimum, signing, notarization, and native Programs acceptance remain unverified. No download link or replacement build is available from this run.
