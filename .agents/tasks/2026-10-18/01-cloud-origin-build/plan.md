# Change the cloud origin and build Zmux

**Date:** 2026-10-18
**Task:** 01-cloud-origin-build

## Objective
Replace the configured cloud hostname with `zmux.w3api.dev` and publish a new
macOS development build through the existing GitHub Actions workflow.

## Context
- The user authorized the hostname replacement and GitHub Actions build.
- Preserve unrelated dirty runtime, documentation and profile-lifecycle work.
- Native packaging reads `config/deployment.json`; Swift also has a fallback.
- Local Compose and its health receipt carry the public auth origin separately.
- Prior native run 39070129651 built the app but failed its stale v9 saved-session
  assertion; the production session writer and Rust behavior tests require v10.

## Approach
Update active source/configuration/docs and the existing callback fixture inputs.
Retain dated historical evidence under older tasks. Correct the confirmed stale
native schema expectation and reuse the bounded retry policy for the observed
close-time busy refusal, preserving direct observation dispatch. Validate an isolated
snapshot, commit scoped files on main, and push for Actions.
Build the cloud image package as well; it remains a package, not a deployment.
Do not change Cloudflare routing or restart the serving local service in this task.

## Steps
- [x] Inspect hostname consumers, existing task evidence and build failures.
- [x] Update active hostname consumers and stale native schema expectation.
- [x] Run required portable checks against the scoped snapshot.
- [x] Commit/push scoped changes and verify exact remote revision.
- [x] Obtain GitHub native and cloud build results and artifact links.

## Acceptance criteria
- [x] Active product/configuration/docs use `https://zmux.w3api.dev`.
- [x] Portable checks pass and unrelated dirty work remains intact.
- [x] GitHub Actions produces the requested downloadable build at the new SHA.
- [x] Signing, native validation and live service activation limits are recorded.
