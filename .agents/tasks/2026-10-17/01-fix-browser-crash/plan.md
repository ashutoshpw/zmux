# Diagnose and fix the desktop sign-in crash

**Date:** 2026-10-17
**Task:** 01-fix-browser-crash

## Objective
Fix the reported desktop authentication callback crash, validate it at its native
owner, and publish a replacement macOS build through GitHub Actions.

## Context
- The 2026-10-17 report omitted the crashed thread. The retained-window guard was
  published as `4b9c4b8`; the new full report identifies a different, exact cause.
- On 2026-10-19, build 217 (source `380c6f7`) traps in signIn's completion handler
  on SafariLaunchAgent's XPC queue, before its inner main-actor Task can start.
- Preserve the existing cloud activation status edit and profile-lifecycle work.
- The running cloud server is healthy at the new origin; this fix is desktop-only.

## Approach
Make the legacy SDK completion explicitly Sendable, retaining the main-actor hop
and generation fencing. Display the SDK error instead of hiding its explanation.
Compile the actual controller against a foreign-queue SDK transport stand-in;
prove the reported build's source traps and the current source recovers its UI.
No production-only test seam, browser session, credentials or HTTP requests needed.

## Steps
- [x] Correlate build 217, full crashed stack and current production callback.
- [x] Fix callback isolation and add a native owner regression with baseline proof.
- [ ] Validate portable checks, hosted baseline/current regression and native app.
- [ ] Commit/push scoped files and deliver a verified replacement build.

## Acceptance criteria
- [ ] Pre-fix callback reproduces the executor trap on a background queue.
- [ ] Fixed callback restores UI safely and reports the SDK failure on MainActor.
- [ ] Required portable and hosted native checks pass; source/build receipt verified.
- [ ] Real Safari launch and authenticated login remain clearly distinguished from
      deterministic callback regression coverage and require the affected Mac retry.

## References
- [Callback crash evidence](references/authentication-callback.md) — full-stack
  diagnosis, test boundary and validation evidence.
