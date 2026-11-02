# Provider build delivery evidence

**Updated:** 2026-11-02
**Source:** `b1e302b47004a3d39596e426650f9248aa1e3115`

## Result

**BLOCKED; no accepted universal macOS app artifact is available for this source.**

The exact-source Foundation workflow passed on Ubuntu but failed on macOS in
`Check existing Rust modules and foreign C ABI`. The complete failed-job log
was retrieved from GitHub. The primary failure is the new long-`TMPDIR` local
Codex route regression timing out before the fixture provider started; the log
does not establish an infrastructure-only cause, so no same-source retry was
started. The exact-source macOS development workflow was still validating its
Rust core at the last check. Its only artifacts were scheduler helpers/layout;
there was no universal app artifact to accept or inspect.

## Current exact-source workflows

- [Foundation checks run 41447789115](https://github.com/ashutoshpw/zmux/actions/runs/41447789115), source SHA `b1e302b47004a3d39596e426650f9248aa1e3115`.
  - `foundation (ubuntu-latest)` passed.
  - `foundation (macos-latest)` failed at **Check existing Rust modules and foreign C ABI** (job `125285109788`).
- [macOS development build run 41447789186](https://github.com/ashutoshpw/zmux/actions/runs/41447789186), same source SHA.
  - Both Linux scheduler jobs and `swift-typecheck` passed.
  - At the last check, `development` was in **Validate Rust core and foreign ABI**. Its build, packaged smoke, sidebar acceptance, and app-artifact upload steps had not run.
  - The listed artifacts were scheduler helpers/layout only; no `zmux-macos-dev-universal-b1e302b47004a3d39596e426650f9248aa1e3115` artifact was present.

## Failure and diagnostics

In Foundation's `apps/cli/tests/provider_hooks.rs`,
`automatic_local_codex_uses_its_exact_real_desktop_terminal_route` failed at
line 1760. Its isolated child failed at line 1891 after the wrapper printed:

```text
zmux: automatic codex tracking unavailable (PR lookup deadline; metadata was not changed); launching once without lifecycle adapter
```

The expected `CODEX_FIXTURE_READY` never appeared before the parent's 10-second
terminal-output deadline. The remaining 12 provider-hook failures were
cascades: their shared test mutex was poisoned after the primary test failed
(`provider_hooks.rs:204`). The complete job log contains 698 lines and was
retrieved with the GitHub workflow-job logs connector. It does not identify
why the Desktop route lookup reached its deadline.

Local `RUST_TEST_THREADS=2 scripts/check` r6 passed at
`/tmp/zmux-provider-support-check-r6.log`; the long-`TMPDIR` regression passed
there. This local result does not replace the failed hosted macOS Foundation
gate.

Scoped path review found no normal ownership or cleanup regression. Setup
failure paths remove the fresh invocation directory; normal provider exit
stops and joins its listener, then removes only that directory. The exact
verified session root remains held. The fake Codex WebSocket fixture removes
its private `/tmp` directory after joining the server. Forced process
termination can leave private invocation files under `/tmp`, as already
documented in `provider-adapters.md`.

No candidate app artifact is accepted. In particular, no app ZIP checksum,
packaged source metadata, architectures, minimum OS, signing receipt, packaged
sidebar/status smoke, or screenshot was available to verify. Authenticated
provider turns, physical Mac/audio behavior, and Intel runtime behavior remain
unverified.

## Previous failed source: `8523529549da19776947d8124b03eda27aaf3a21`

- Foundation run [41445123588](https://github.com/ashutoshpw/zmux/actions/runs/41445123588) failed on macOS because `existing_daemon_websocket_is_bounded_and_queues_interleaved_notifications` could not bind the fake socket: `path must be shorter than SUN_LEN`. The complete log was `/tmp/zmux-foundation-41445123588-macos.log` (SHA-256 `7fc0d7bec072494d4db930ae297d63798a60b9f8de8ccf23a8334984606303e`).
- Development run [41445123728](https://github.com/ashutoshpw/zmux/actions/runs/41445123728) passed Swift typechecking but did not produce a completed universal app artifact.
- The old fixture used `env::temp_dir()` with a UUID/socket suffix; the hosted macOS `TMPDIR` made that path too long. The fixture correction now uses canonical `/tmp`.
- Local `RUST_TEST_THREADS=2 scripts/check` r4 passed at `/tmp/zmux-provider-support-check-r4.log`, but no app ZIP, packaging receipt, or accepted artifact came from that source.
