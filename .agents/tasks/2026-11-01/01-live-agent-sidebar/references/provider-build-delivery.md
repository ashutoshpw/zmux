# Provider build delivery evidence

**Updated:** 2026-11-02
**Source:** `5559f09b2c4b3ee12baa345dcd5ffc126ed74b3c`

## Result

**BLOCKED: there is no accepted universal macOS app artifact for this exact source.**

The automatic push started both required workflows on the exact pushed SHA. Ubuntu Foundation and standalone Swift typechecking passed, but the hosted macOS Rust checks failed in the same Codex provider-hook fixture. The universal build and packaged acceptance steps were skipped. No same-source retry was started because the available evidence is a source/test failure, not a definitive infrastructure transient.

## Exact-source workflows

- [Foundation checks run 41450440329](https://github.com/ashutoshpw/zmux/actions/runs/41450440329), source SHA `5559f09b2c4b3ee12baa345dcd5ffc126ed74b3c).
  - Ubuntu passed **Check existing Rust modules and foreign C ABI** and **Check browser locator and readiness DOM behavior**.
  - macOS failed **Check existing Rust modules and foreign C ABI** in [job 125294023870](https://github.com/ashutoshpw/zmux/actions/runs/41450440329/job/125294023870).
  - Primary failure: `automatic_local_codex_uses_its_exact_real_desktop_terminal_route_child` reached the lifecycle baseline wait, but `wait_local_codex` timed out at `apps/cli/tests/provider_hooks.rs:139` with `local Codex baseline did not attach to the existing root thread; provider snapshot=[]`. The parent failure is at line 1760. The fixture had passed its explicit daemon setup and reached `CODEX_FIXTURE_READY`; the run does not show why the lifecycle record was absent.
  - Twelve subsequent provider-hook failures were poisoned-mutex cascades and are not separate findings.

- [macOS development build run 41450440458](https://github.com/ashutoshpw/zmux/actions/runs/41450440458), same source SHA.
  - `swift-typecheck`, both Linux scheduler builds, scheduler layout validation, and the development job's preceding native source/fixture validations passed.
  - The development job failed **Validate Rust core and foreign ABI** in [job 125295521666](https://github.com/ashutoshpw/zmux/actions/runs/41450440458/job/125295521666) on the same empty local Codex lifecycle snapshot. Universal compilation, candidate retention, packaged terminal/native/sidebar checks, and final app upload were skipped.
  - The run exposes only scheduler queue layout and Linux scheduler helper artifacts. It has no candidate or `zmux-macos-dev-universal-5559f09b2c4b3ee12baa345dcd5ffc126ed74b3c` artifact.

The local `RUST_TEST_THREADS=2 scripts/check r7` passed at `/tmp/zmux-provider-support-check-r7.log`, and the source reviews were accepted. Those checks do not override the failed hosted macOS gate.

## Fixture correction recheck

The scoped correction faithfully models the production precondition that a shared Codex daemon already exists before the status probe:

- The local and managed-SSH synthetic terminal commands explicitly start the fixture daemon. Fixture setup has its own bounded ten-second readiness phase; normal `app-server daemon version` only reports running or stopped and does not start a daemon.
- Production `RPC_TIMEOUT` remains two seconds. Only the probe error text changed to identify Codex status; the production deadline and RPC behavior did not change.
- The patch changes fixture startup commands and the local outer screen-output allowance. It does not alter lifecycle, exact-owner, foreign-thread, Stop-hook, or once-only completion assertions. The hosted local test failed before reaching those later assertions. The passing local r7 check covered the local and managed-SSH Codex regressions.

The earlier failure to reach `CODEX_FIXTURE_READY` is no longer the observed failure. The current hosted logs establish that fixture setup progressed, but do not identify why the baseline was empty; this needs source-level diagnosis before another push.

## Artifact and proof limits

No app ZIP, checksum sidecar, build metadata, packaged source receipt, architecture/minimum-OS receipt, signing or notarization receipt, executed packaged native smoke, sidebar/status receipt, or sidebar screenshot exists for this source. Therefore none of those fields can be accepted or reported for `5559f09`.

The synthetic fixtures do not establish an authenticated Codex/OpenCode provider turn. Physical Mac use, audible real-provider events, and Intel runtime behavior remain unverified.

## Previous failed-source history

- Source `8523529549da19776947d8124b03eda27aaf3a21`: Foundation run [41445123588](https://github.com/ashutoshpw/zmux/actions/runs/41445123588) failed when a Darwin Unix-socket fixture path exceeded `SUN_LEN`; development run [41445123728](https://github.com/ashutoshpw/zmux/actions/runs/41445123728) produced no accepted universal app. The short private `/tmp` fixture path was added afterward.
- Source `b1e302b47004a3d39596e426650f9248aa1e3115`: Ubuntu Foundation passed; macOS Foundation [41447789115](https://github.com/ashutoshpw/zmux/actions/runs/41447789115) and development [41447789186](https://github.com/ashutoshpw/zmux/actions/runs/41447789186) failed while the synthetic daemon cold-start consumed the bounded status-probe interval. The current fixture now starts the daemon explicitly before the adapter invocation.
