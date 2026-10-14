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
