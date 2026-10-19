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
- The fixture compiles actual CloudSettings.swift and CloudClient.swift unchanged,
  against the real imported AuthenticationServices class and API annotations.
  A fixture-only Objective-C category captures the callback in the real initializer
  and suppresses `start`; no browser or HTTP authentication session is launched.
  It invokes that actual Objective-C block on a background queue and inspects the
  real sign-in button and message. Only the unrelated app-shell owner is a stand-in.
  No production test seam or duplicated authentication implementation is added.
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

## First hosted regression result
- Native run 39211112615 passed production source type-checking and existing cloud
  credential checks, then correctly rejected the first regression harness: its
  Swift replacement of the framework initializer did not reproduce the actor trap.
- That replacement omitted the actual imported Objective-C block boundary, changing
  Swift's dynamic isolation enforcement. Replaced it with capture of the real SDK
  completion via a fixture-only initializer exchange. The original initializer
  runs, `start` is suppressed, and methods/captured blocks are reset on completion.
- Production fix is unchanged; portable validation still applies to that source.
  New fixture Objective-C compilation and baseline/current proof need hosted macOS.

## Successful native proof and delivery
- Run 39212506138, build 220, source d595a903cc53bc1c012197e9d67ede8a97698bec
  passed. The callback step logs both the baseline trap and the fixed controller's
  successful MainActor UI recovery/error presentation. Full configured native gates
  passed and artifact 12169401068 was downloaded and inspected.
- Embedded Info.plist confirms build 220 and https://zmux.w3api.dev; the ZIP
  checksum matches its manifest. Real Safari launch and authenticated production
  sign-in on the affected Mac remain a retry, distinct from the deterministic test.
