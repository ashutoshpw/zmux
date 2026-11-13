# Hosted acceptance: `6230bd4`

**Source:** `6230bd4fb6842d15901f1c02daab7fa91dfb9dea` (`main`)
**Scope:** Observe the existing Foundation and universal development workflows. No dispatch, retry, cancellation, local checks, app launch, source/status/plan/index edit, commit, or push.

## First primary failure: Foundation macOS

Foundation run [43398620022](https://github.com/ashutoshpw/zmux/actions/runs/43398620022) reached terminal **failure**. Ubuntu completed successfully. The macOS job [131795589236](https://github.com/ashutoshpw/zmux/actions/runs/43398620022/job/131795589236) failed in **Check existing Rust modules and foreign C ABI**, running `scripts/check`.

The failing test was `managed_ssh_codex_uses_root_thread_events_and_ignores_stop_hook_decisions` in `apps/cli/tests/ssh.rs:166:13`:

```text
test managed_ssh_codex_uses_root_thread_events_and_ignores_stop_hook_decisions ... FAILED
thread 'managed_ssh_codex_uses_root_thread_events_and_ignores_stop_hook_decisions' panicked at apps/cli/tests/ssh.rs:166:13:
remote terminal did not reach expected state: {"bytes_read":3098,...,"state":"running",...}
zmux: managed SSH Codex tracking unavailable (Desktop provider-route admission was not confirmed); launching once without lifecycle tracking
test result: FAILED. 2 passed; 1 failed; 0 ignored; 0 measured; 0 filtered out
Process completed with exit code 101.
```

The failure output also records lifecycle cancellation `EPERM: Operation not permitted` and two lifecycle startup retries after the stream closed with `hello=false, active=false`. The complete terminal snapshot and log are preserved; no root cause is inferred here.

The full decoded failed-job log is `/tmp/zmux-osc7501-final-6230bd4/foundation-macos-failed-job-full.log` (**53,651 bytes**, SHA-256 `b01d36619b7151b3df37aed699de0cfbc3d63d10bb3f8834f7238d3b5f412af3`). The Foundation Artifacts API returned `artifacts: []`; the snapshot is `/tmp/zmux-osc7501-final-6230bd4/foundation-run-artifacts-api.json`. No diagnostic archive was published, so there was no archive to download or CRC-check/extract.

## Universal development run

Run [43398619986](https://github.com/ashutoshpw/zmux/actions/runs/43398619986) was still in progress when the Foundation primary failure was found. Both Linux scheduler jobs and Swift type-check had passed. The development job was in **Validate Rust core and foreign ABI**; app build, native acceptance, and final artifact upload had not yet been observed. Observation stopped at the first primary failure as requested, so the universal run's later outcome and final artifact acceptance are unverified here.

## Preserved evidence and boundary

- Parent-supplied local `scripts/check` remains **763 passed, 0 failed, 7 ignored in 263.27 seconds**; it was not rerun.
- Retained Ghostty linkage remains the existing accepted run `43371114352` / artifact `14012631713` at `2e2c9d3`, with parent-supplied verification that all 229 source pins match this source. No fresh linkage build or new acceptance claim was made.
- The index was left untouched, and this reference was the only checkout path changed by this observation. Pre-existing dirty paths were preserved. Diagnostics added under `/tmp/zmux-osc7501-final-6230bd4/` are the failed-job log and empty-artifact-list snapshot.

## Parent universal follow-up

Universal run later failed its aggregate in
`managed_ssh_shell_routes_nonce_bound_opener_requests_from_loopback_sshd`,
`crates/remote-runtime/tests/connection.rs:549`: the exact event receipt socket
read returned WouldBlock (macOS errno 35). The connection target reported 8
passed, 1 failed. Complete development job 131797064381 log is retained at
`/tmp/zmux-osc7501-final-6230bd4/universal-failed-job-full.log`. App build and
native checks were skipped. This differs from Foundation's route admission
fallback and does not yet establish a shared cause.
