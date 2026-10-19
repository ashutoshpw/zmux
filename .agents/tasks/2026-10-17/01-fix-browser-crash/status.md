# Status

- **State:** in-progress
- **Updated:** 2026-10-19

## Current follow-up
- Full build-217 report confirms an executor assertion at the entry of signIn's
  actor-inherited completion on SafariLaunchAgent's background queue.
- Explicit Sendable callback and clearer SDK failure presentation implemented;
  native actual-controller baseline/current regression is wired before Rust builds.
- `RUST_TEST_THREADS=2 scripts/check` passed against an isolated HEAD export plus
  this task: workspace tests, Clippy, license inventory, C ABI/profile lease checks.
- Python driver parses, shell entrypoint syntax and `git diff --check` pass.
- Local host has no Apple SDK. Native regression and replacement artifact pending.
- Fix commit `deb76acc6dd7235e6855232f9e18712d69df6b4a` pushed to origin/main;
  exact remote SHA parity verified. Hosted run:
  https://github.com/ashutoshpw/zmux/actions/runs/39211112615

## Prior investigation (2026-10-17)
- Matched app build 209 to macOS workflow run `38948679333` and source commit `1f122f4`; hosted build and packaged checks passed.
- The supplied excerpt omits Thread 17's stack and Binary Images. User reports the crash happened while opening cloud account login.
- Cloud login uses `ASWebAuthenticationSession` and provides `window!` from `presentationAnchor(for:)`; this callback aligns with the SafariLaunchAgent queue. Replaced that force unwrap with a strongly retained exact window anchor.
- `RUST_TEST_THREADS=2 scripts/check` passed, including Rust browser/profile suites and C ABI checks.
- This environment cannot compile/run AppKit or AuthenticationServices. Hosted macOS build and a real login retry remain outstanding.
- Fast-forwarded local `main` to current `origin/main` (`30ed3c1`) without touching existing untracked profile-lifecycle work.

## Regression harness correction
- Native run 39211112615 passed the production Swift source checks. The first
  baseline harness failed its reproduction gate: a Swift stand-in changed callback
  isolation inference and did not trap. This is not accepted as regression proof.
- The revised fixture captures the real SDK-typed Objective-C completion while
  suppressing browser launch. The product fix is unchanged; portable checks remain
  passed. Native fixture compilation and before/after execution are pending.
- Run 39212106241 again passed production type-checking, then stopped compiling
  the Objective-C fixture because NS_METHOD_FAMILY was not defined by this SDK.
  Replaced it with Clang's objc_method_family(init) attribute; also aligned the
  fixture's target/debug options with the packaged macOS 14 app. Product unchanged.
