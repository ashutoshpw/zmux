# Provider build delivery evidence

**Updated:** 2026-11-02

**Source:** `de857fc8e36f09fd3744b6b157f285d2265b1b0d`

## Latest exact-source result

**BLOCKED: no accepted universal macOS app artifact exists for this source.** The exact-source Foundation and macOS development workflows both failed in the same macOS Codex provider-hook fixture. Ubuntu Foundation, standalone Swift typechecking, and both Linux scheduler builds passed. Development packaging and packaged acceptance were skipped after the Rust-core gate failed. No retry was started because this is a reproducible source/test failure, not an infrastructure failure.

- [Foundation checks run 41453473961](https://github.com/ashutoshpw/zmux/actions/runs/41453473961), source SHA `de857fc8e36f09fd3744b6b157f285d2265b1b0d`.
  - Ubuntu job [125304208616](https://github.com/ashutoshpw/zmux/actions/runs/41453473961/job/125304208616) passed.
  - macOS job [125304208202](https://github.com/ashutoshpw/zmux/actions/runs/41453473961/job/125304208202) failed **Check existing Rust modules and foreign C ABI**. Full job log: `/tmp/zmux-provider-foundation-mac-de857.log`.
- [macOS development build run 41453473964](https://github.com/ashutoshpw/zmux/actions/runs/41453473964), same source SHA.
  - Both Linux scheduler jobs and `swift-typecheck` passed.
  - Development job [125305843196](https://github.com/ashutoshpw/zmux/actions/runs/41453473964/job/125305843196) failed **Validate Rust core and foreign ABI** on the same fixture. Full job log: `/tmp/zmux-provider-development-de857.log`.
  - Universal compilation, candidate retention, packaged terminal/native/sidebar checks, and downloadable development build upload were skipped. The run's only artifacts are scheduler helpers and queue-layout evidence; no `zmux-macos-dev-universal-de857fc8e36f09fd3744b6b157f285d2265b1b0d` artifact exists.

## Failure diagnosis

The primary failure is `automatic_local_codex_uses_its_exact_real_desktop_terminal_route_child`, timing out at `apps/cli/tests/provider_hooks.rs:139` with an empty provider snapshot. The captured bounded terminal text in both macOS runs is:

```text
{"status": "running", "socketPath": "/private/tmp/zhooks-<fixture>/codex-app-server.sock"}
CODEX_FIXTURE_READY
zmux: provider event route was lost; activity remains Unknown
zmux: Codex SessionStart tracking is not active; review the Zmux hook in /hooks. No lifecycle state is being inferred.
```

This narrows the failure to the local provider-event ingress path before a Codex baseline was observed. In `provider-synthetic-codex.py`, the fixture prints `CODEX_FIXTURE_READY` only after its SessionStart hook subprocess returns exit code zero (`apps/cli/tests/fixtures/provider-synthetic-codex.py:295-311, 339-350`). That subprocess invokes the CLI hook; `send_local_event` connects and writes one bounded JSON line to the invocation socket (`apps/cli/src/hooks/provider.rs:139-150, 192-208`). A successful hook return proves the fixture reached that write path, but does not prove the listener accepted and decoded the event.

The `provider event route was lost` message is emitted when `listener_lost` is set (`apps/cli/src/hooks/provider.rs:600-612`). The listener currently collapses accepted-stream setup, bounded frame read, JSON/schema validation, queue saturation, and listener-accept failures into the same loss flag without retaining the failure class (`apps/cli/src/hooks/provider.rs:460-529, 531-551`). The later SessionStart warning and absence of `Codex thread could not be attached...` distinguish this from the watcher attach/snapshot error branch (`apps/cli/src/hooks/provider.rs:618-635, 691-698`). Therefore the evidence identifies event-listener ingress as the failed stage; it does not identify which listener operation failed.

The exact Codex attach and Desktop admission path follows only after a `CodexSession` event is received: `Watcher::attach` connects to the existing daemon, reads the exact thread snapshot, and publishes its baseline (`apps/cli/src/hooks/provider/codex.rs:289-328`). Publication then submits a provider observation (`apps/cli/src/hooks/provider/codex.rs:533-560; apps/cli/src/hooks/provider.rs:722-752`), which the Desktop applies only to the exact current local terminal and running session (`crates/desktop-runtime/src/lib.rs:2960-3017`). The log contains no attach error or Desktop admission rejection. It does not support attributing this failure to the Codex WebSocket snapshot or the Desktop lifecycle owner.

The concrete cause established by hosted evidence is a lost local event-listener route before the Codex baseline appeared. The lower-level listener cause remains unresolved because `accept_events` discards it. The minimal next repair is to retain and report a bounded sanitized listener failure class (accept, accepted-stream setup, frame read/deadline, frame decode/validation, or queue full), without including provider RPC payloads; then run the existing exact-identity fixture against that diagnosis. Keep provider state/identity assertions and production deadlines unchanged. This is a repair proposal only; this worker made no source, test, or workflow edits.

The prior source `5559f09` failure also had an empty baseline after `CODEX_FIXTURE_READY`. It is still reproducible on `de857fc`; the diagnostics now locate the failure at the local event-listener stage but do not explain the listener operation. This is unresolved reliability evidence, not a repaired root cause. Poisoned-mutex failures after the primary fixture panic are cascades, not separate defects.

## Verification and limits

- `RUST_TEST_THREADS=2 scripts/check r8` passed locally before this source was pushed: `/tmp/zmux-provider-support-check-r8.log`.
- Both exact-source macOS runs failed the Codex local provider-hook fixture. Exact-source Foundation Ubuntu passed; the macOS development run's Swift typecheck and Linux scheduler builds passed. App packaging and packaged smoke did not execute.
- No accepted app ZIP, app checksum, packaged source/build metadata, architecture/minimum-OS receipt, signing/notarization receipt, native sidebar/status/selection evidence, or sidebar screenshot exists for this source.
- The synthetic fixture does not prove authenticated provider turns, live model behavior, physical Mac/audio behavior, or Intel runtime behavior.
- Worktree source remained at `de857fc8e36f09fd3744b6b157f285d2265b1b0d`; unrelated cloud-origin/profile-lifecycle changes were preserved.

## Previous failed-source history

- Source `8523529549da19776947d8124b03eda27aaf3a21`: Foundation run [41445123588](https://github.com/ashutoshpw/zmux/actions/runs/41445123588) failed when a Darwin Unix-socket fixture path exceeded `SUN_LEN`; development run [41445123728](https://github.com/ashutoshpw/zmux/actions/runs/41445123728) produced no accepted universal app. The short private `/tmp` fixture path was added afterward.
- Source `b1e302b47004a3d39596e426650f9248aa1e3115`: Ubuntu Foundation passed; macOS Foundation [41447789115](https://github.com/ashutoshpw/zmux/actions/runs/41447789115) and development [41447789186](https://github.com/ashutoshpw/zmux/actions/runs/41447789186) failed while the synthetic daemon cold-start consumed the bounded status-probe interval. The fixture was changed to start the daemon explicitly before adapter invocation; production status deadlines were unchanged.
- Source `5559f09b2c4b3ee12baa345dcd5ffc126ed74b3c`: Foundation [41450440329](https://github.com/ashutoshpw/zmux/actions/runs/41450440329) and development [41450440458](https://github.com/ashutoshpw/zmux/actions/runs/41450440458) reached `CODEX_FIXTURE_READY`, then failed the empty local Codex baseline assertion. No cause was visible in that source's diagnostics, so no speculative timeout or ownership change was made.
