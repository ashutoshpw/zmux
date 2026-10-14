# Downloadable native macOS development build

**Date:** 2026-10-14
**Task:** 03-macos-development-build

## Objective
Deliver an actual interactive macOS development app through GitHub Actions artifacts,
with native rendering and Rust-owned local sessions. A successful hosted build and artifact
are required; a workflow definition or headless prototype alone does not satisfy this task.

## Context
- User explicitly requested a downloadable development build from GitHub Actions.
- Publishing commits/workflow and running hosted macOS validation are authorized by that request.
- Remote repository is empty; local `main` contains the independently authored MIT foundation.
- Full cloud/remote/iOS and cmux PR #1 scope remains in the project plan.

## Approach
Add a bounded Rust desktop coordinator, terminal rendering runs, native Swift/AppKit app,
and reproducible universal arm64/x86_64 packaging. Ad-hoc sign the development bundle,
include source revision/notices, exercise the packaged app on macOS, and upload the ZIP.

## Steps
- [x] Commit Rust desktop session/action integration and public behavioral checks.
- [ ] Implement the Swift native host, input/IME, workspace/pane views and teardown.
- [ ] Add universal build, bundle validation, native smoke checks and artifact workflow.
- [ ] Push `main`, repair actual hosted failures, and verify downloadable artifacts.
- [ ] Record the exact successful SHA/run/artifact and remaining product gates.

## Acceptance criteria
- [ ] Native app launches a real local shell and displays parsed terminal color/text/cursor state.
- [ ] Workspace switch, pane split/close, input/paste and resize use Rust-owned operations.
- [ ] PTY work stays off the UI thread; startup/close races and limits are covered.
- [ ] Bundle contains both architectures, runtime library, MIT/third-party notices and build metadata.
- [ ] Packaged native smoke check exercises shell input, splitting, resize and rendering.
- [ ] GitHub Actions publishes a downloadable `.app` ZIP for the current successful SHA.
- [ ] Ad-hoc signing, missing notarization, manual dogfood and remaining parity are documented accurately.

## References
- [Implementation evidence](references/implementation.md) — checks, hosted run and delivery limits.
