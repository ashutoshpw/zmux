# Zmux contributor instructions

- Read the current task under `.agents/tasks/` and update evidence/status with completed slices.
- Original Zmux code is MIT. Disclose imported sources and retain dependency/asset notices.
  Do not import or translate cmux GPL/BUSL implementation or assets into this repository.
- Rust owns workspace/session logic. Swift/AppKit owns native presentation; share actions.
- Keep PTY/network/process waits off the UI thread and core action executor. Bound queues
  and scrollback; preserve terminal byte ordering and make overload/recovery explicit.
- Test observable behavior at its production owner. Run `scripts/check` before committing;
  record unavailable native/hosted gates. Never bypass commit hooks or disable failing tests.
- Commit cohesive validated slices on `main`. If a branch is necessary, create its worktree
  under `~/.worktrees/ashutoshpw/zmux/<branch-name>/`, or `pr-<number>/` for PR work.
  Use a local tracking branch when the matching remote branch exists; avoid detached HEAD.
- Do not push, deploy, provision paid infrastructure, or modify the user's running cmux
  unless that action is authorized. A portable prototype is not a native release.

## Required build delivery

- Every user-requested bug fix or feature addition/change includes a fresh downloadable
  build by default, in every chat. Do not wait for a separate request for the artifact.
- This is standing authorization to commit and push the scoped, validated changes to
  `origin/main` and run the existing build workflows needed for delivery. Preserve unrelated
  work. It does not authorize production deployments, paid infrastructure, release
  publication, or replacing/quitting the user's running apps.
- Default to the universal macOS build for Apple Silicon and Intel; deliver another
  platform's build when the task or user specifies that target. Follow
  [development-build instructions](docs/development-builds.md) and existing workflows.
- Continue through the relevant executed checks, packaging and artifact upload. A started
  workflow, skipped check, candidate archive or older build does not establish completion.
  Diagnose failures within scope and finish the delivery; if genuinely blocked, state the
  exact blocker and distinguish any unverified candidate from an accepted build.
- Download the final artifact and verify its checksum, packaged source SHA/build metadata,
  target architectures and the task's acceptance results. For UI changes, inspect the
  actual packaged-app visual evidence when available. Record the delivery evidence in the
  current task. Never claim physical-device or production acceptance from hosted checks.
- The final response must include the artifact download link, source commit, target/minimum
  OS, relevant verification and signing/notarization status. Remind the user to quit the old
  Zmux before opening a replacement Mac build.
- Read-only investigations and documentation/instruction-only changes do not require an app
  rebuild unless requested. Explicit user instructions can override this default workflow.
