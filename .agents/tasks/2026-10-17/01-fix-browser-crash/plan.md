# Diagnose and fix Safari/WebKit crash

**Date:** 2026-10-17
**Task:** 01-fix-browser-crash

## Objective
Identify the Zmux code path implicated in the SafariLaunchAgent crash report, implement a targeted fix, validate it, and push the fix to `origin/main`.

## Context
The provided report excerpt omits Thread 17's backtrace. The existing checkout is behind `origin/main` and contains unrelated untracked profile-lifecycle work that must be preserved.

## Approach
Update source context from `origin/main`, inspect browser lifecycle and recent WebKit changes, and only implement a fix supported by source or crash evidence. Keep the commit scoped to this task.

## Steps
- [x] Refresh source and correlate browser lifecycle with the crash.
- [x] Correlate the reported login action with `ASWebAuthenticationSession` and its Safari LaunchAgent presentation callback.
- [x] Retain and return the Cloud settings window directly as the authentication presentation anchor, removing the force unwrap of `NSWindowController.window`.
- [x] Run required portable validation and inspect the scoped diff; record that native Swift/WebKit checks require macOS.
- [ ] Commit the fix on `main` and push to `origin/main`.

## Acceptance criteria
- [x] The reported Cloud login path and Safari LaunchAgent queue are connected to the presentation anchor callback; that callback no longer force-unwraps an optional window.
- [x] `RUST_TEST_THREADS=2 scripts/check` passes, with native gates recorded if unavailable.
- [ ] The scoped commit is present on `origin/main`; unrelated untracked work remains untouched.
- [ ] Hosted macOS build validates the Swift change and packaged login flow is dogfooded.

## Evidence so far
- Build `0.2.0 (209)` is macOS development workflow run `38948679333`, built from `1f122f423dabd3a7cd3a94381523eb957334d4b4`; the hosted run passed.
- The report excerpt omits Thread 17's frames and Binary Images. The trigger was opening the cloud account login browser. Cloud login uses `ASWebAuthenticationSession`, whose presentation callback is requested while the Safari authentication UI launches.
- Source at that SHA uses `ASWebAuthenticationSession` for cloud account login; the provider returns the presentation window during Safari's authentication UI launch.
- The directly implicated app callback force-unwrapped `NSWindowController.window`; it now returns a strongly retained, exact Cloud settings window instead.
- Focused audit of build 209's browser navigation, download, automation, and profile-store code found no app-authored `fatalError`, `precondition`, `try!`, or obvious force unwrap on those paths. This does not rule out an OS/WebKit trap or a timing-dependent lifecycle defect.
- Local `main` was fast-forwarded to `origin/main` (`30ed3c1`). Existing untracked profile-lifecycle files and the new plan remain uncommitted and untouched.
