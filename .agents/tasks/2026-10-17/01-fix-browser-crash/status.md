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

## Prior investigation (2026-10-17)
- Matched app build 209 to macOS workflow run `38948679333` and source commit `1f122f4`; hosted build and packaged checks passed.
- The supplied excerpt omits Thread 17's stack and Binary Images. User reports the crash happened while opening cloud account login.
- Cloud login uses `ASWebAuthenticationSession` and provides `window!` from `presentationAnchor(for:)`; this callback aligns with the SafariLaunchAgent queue. Replaced that force unwrap with a strongly retained exact window anchor.
- `RUST_TEST_THREADS=2 scripts/check` passed, including Rust browser/profile suites and C ABI checks.
- This environment cannot compile/run AppKit or AuthenticationServices. Hosted macOS build and a real login retry remain outstanding.
- Fast-forwarded local `main` to current `origin/main` (`30ed3c1`) without touching existing untracked profile-lifecycle work.
