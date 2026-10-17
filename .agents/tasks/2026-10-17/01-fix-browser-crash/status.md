# Status

- **State:** in-progress
- **Updated:** 2026-10-17

## Notes
- Matched app build 209 to macOS workflow run `38948679333` and source commit `1f122f4`; hosted build and packaged checks passed.
- The supplied excerpt omits Thread 17's stack and Binary Images. User reports the crash happened while opening cloud account login.
- Cloud login uses `ASWebAuthenticationSession` and provides `window!` from `presentationAnchor(for:)`; this callback aligns with the SafariLaunchAgent queue. Replaced that force unwrap with a strongly retained exact window anchor.
- `RUST_TEST_THREADS=2 scripts/check` passed, including Rust browser/profile suites and C ABI checks.
- This environment cannot compile/run AppKit or AuthenticationServices. Hosted macOS build and a real login retry remain outstanding.
- Fast-forwarded local `main` to current `origin/main` (`30ed3c1`) without touching existing untracked profile-lifecycle work.
