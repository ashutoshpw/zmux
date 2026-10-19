# Status

- **State:** done
- **Updated:** 2026-10-19

## Delivery
- [Native build 220](https://github.com/ashutoshpw/zmux/actions/runs/39212506138)
  passed every configured gate: both source slices, actual-SDK callback regression,
  cloud credential checks, Rust/C ABI, packaged app rendering and sidebar/status menu.
- [Download artifact 12169401068](https://github.com/ashutoshpw/zmux/actions/runs/39212506138/artifacts/12169401068):
  `zmux-dev-universal-d595a90.zip`, macOS 14+, arm64/x86_64, ad-hoc signed and
  not notarized. Downloaded and verified source revision, architectures, embedded
  Info.plist build number 220, `https://zmux.w3api.dev`, and checksum manifest.
- Artifact source: `d595a903cc53bc1c012197e9d67ede8a97698bec`.
  ZIP SHA-256: `00b525d7bb353a595cfba56ab2e5776f41083877646fbe7b4166deec82b69cd8`.
- Hosted regression log confirms: baseline actual sign-in callback reproduced
  the foreign-queue actor trap; current actual callback returned, UI recovered on
  MainActor, and the SDK failure was displayed. This addresses the provided stack.
- Actual Safari launch and authenticated sign-in on the affected Mac remain a
  user retry. The regression suppresses SDK start and invokes its real completion
  from a controlled background queue. The existing workflow also explicitly
  defers accessibility Save-panel tests; no claim is made for those gates.
- The backend server and Cloudflare routing were not changed by this fix. Existing
  cloud activation status edit and unrelated profile-lifecycle work stay untouched.
- This completion record is a documentation-only follow-up to the validated build.

## Current follow-up
- Full build-217 report confirms an executor assertion at the entry of signIn's
  actor-inherited completion on SafariLaunchAgent's background queue.
- Explicit Sendable callback and clearer SDK failure presentation implemented;
  native actual-controller baseline/current regression is wired before Rust builds.
- `RUST_TEST_THREADS=2 scripts/check` passed against an isolated HEAD export plus
  this task: workspace tests, Clippy, license inventory, C ABI/profile lease checks.
- Python driver parses, shell entrypoint syntax and `git diff --check` pass.
- Local host has no Apple SDK; hosted native validation and replacement build 220 passed (see Delivery).
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

## Verified callback regression
- Commit `d595a903cc53bc1c012197e9d67ede8a97698bec` is pushed and exact remote
  SHA verified. Native run 39212506138 passed both production source slices and
  the actual-SDK callback baseline/current regression.
- The reported build source traps inside the Objective-C callback on the foreign
  queue; the fixed source returns safely, restores the actual sign-in control and
  presents the injected SDK error. This is runtime owner-boundary proof, not a
  source-string assertion. Full native app packaging/acceptance subsequently passed (Delivery above).
- Run: https://github.com/ashutoshpw/zmux/actions/runs/39212506138
