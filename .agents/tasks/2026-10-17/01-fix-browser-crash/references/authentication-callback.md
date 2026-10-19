# Authentication completion isolation

## Current evidence
- User report: build 0.2.0 (217), arm64, macOS 15.7.4.
- GitHub run 39169304133 has run_number 217 and source
  `380c6f7f218ae83b1c64e33954caa99554f94ca8`.
- Crashed thread 22 is SafariLaunchAgent's XPC reply queue. Frames 0-4:
  `_dispatch_assert_queue_fail`, `dispatch_assert_queue`,
  `swift_task_isCurrentExecutorWithFlagsImpl`, then
  `closure #1 in CloudWindowController.signIn(_:)`.
- AuthenticationServices invokes its completion during `_startDryRun:` failure.
  The completion created in an @MainActor method inherits actor isolation even
  though its body only schedules an inner @MainActor Task. The executor check is
  at callback entry; the inner hop cannot prevent it. Explicit @Sendable removes
  that inherited isolation while preserving the required explicit UI hop.
- The prior retained-window guard was published before this full stack existed;
  it is not the cause shown by this report.

## Regression authoring gate
- Independent contract: a framework completion may enter from any queue and must
  reach the real controller's main-actor UI without trapping, preserving SDK errors.
- Credible regression: an actor-inherited legacy callback traps before enqueuing UI.
- Existing native CloudClient HTTP/Keychain checks never invoke the browser-session
  callback, and full app smoke does not initiate real cloud authentication.
- The fixture compiles actual CloudSettings.swift and CloudClient.swift unchanged.
  Only the SDK transport and unrelated app-shell owner are local stand-ins. It
  dispatches the stored actual completion on a background queue and inspects the
  real sign-in button and message. No production test seam or duplicated auth logic.
- The pinned baseline is the actual reported build's CloudSettings.swift. The driver
  requires a fatal trap after its foreign-invocation marker and before the callback
  returns (macOS may log the dispatch assertion only to CrashReporter);
  compilation failure, unrelated crash or timeout does not count as baseline proof.
- Current acceptance requires the real controller to re-enable sign-in and display
  the injected SDK failure. Compilation is strict Swift 6 with warnings as errors
  and release optimization, matching the relevant runtime enforcement.
- This proves callback delivery and UI recovery. It does not open Safari, access
  Keychain, send HTTP requests or establish authenticated production login.

## Validation
- Complete isolated `RUST_TEST_THREADS=2 scripts/check` passes. Driver Python
  parsing, shell syntax and `git diff --check` pass.
- Hosted macOS compilation/baseline-current execution and replacement build pending;
  no local Apple SDK is available.
