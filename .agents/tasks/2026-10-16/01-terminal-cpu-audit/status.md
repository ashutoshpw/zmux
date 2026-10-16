# Status

- **State:** in-progress
- **Updated:** 2026-10-16

## Notes
- Notification phase full portable checks pass (`RUST_TEST_THREADS=2 scripts/check`,
  `/tmp/zmux-claude-shell-check-2.log`), including real SSH transfers/reopen and C ABI.
  A focused manual macOS workflow now validates local provider contract, packaged
  status/sidebar attention and real SSH notification cleanup before app upload.
  Baseline broad workflow38921565012 progressed past v9 persistence and exposed
  a stale icon inventory: compact sidebar buttons were omitted and removed Cloud
  account toolbar control remained expected. Updated actual visible-control
  accessibility inventory; native revalidation is required, not skipped.
- Phase1 native baseline at86aacfe, Actions38921563377, passes both120s phases
  with complete observations: idle1 app CPU0.4333%, idle8 CPU0.4830%. Warmup stack
  captures are dominated by OS wait states. Downloaded receipts and stack hashes
  verified at `/tmp/zmux-idle-baseline-38921563377/`. This does not reproduce the
  physical-Mac31.5%; a user high-CPU sample is pending while independent work proceeds.
- Local completion regression failed before repair and passes with a controlled
  provider executing the real session plugin, while remaining alive. Authenticated
  loopback SSH acceptance passes exact-surface attention and remote hook cleanup.
  Neither invokes Claude's API nor claims a visible OS banner. Regression logs:
  `/tmp/zmux-claude-attention-red.log`, `/tmp/zmux-claude-attention-green.log`,
  `/tmp/zmux-ssh-notification-acceptance.log`. Startup/argument/opt-out checks pass.
- Existing SSH upload regression caught shell storage sharing transfer TMPDIR;
  remote shell lease now uses its own0700 `/tmp/zmux-shell.*` directory. The
  unchanged real authenticated transfer/reopen test passes afterward; no upload
  assertion was relaxed (`/tmp/zmux-ssh-regression-green.log`). Full recheck follows.
- Implementation authorized. Starting opt-in idle stack sampling and shared
  provider lifecycle ingress; SSH remains required in this session. Existing
  untracked profile-lifecycle work is preserved.
- User explicitly requires SSH notification integration in this same session.
  Local and managed-SSH completion journeys both gate the repair artifact; remote
  integration is no longer deferred. This supersedes the earlier scope separation.
- User clarified mostly-idle high CPU including without Claude; the screenshot
  included one active local Claude. Missing completion was local, not SSH. Plan
  prioritizes current-build macOS idle stacks, flat-PTY publication/redraw
  correlation and repair of the measured owner. Active-output amplification is
  not accepted as this idle root cause; remote notifications are separate work.
- Investigation and requested fix plan are prepared; no runtime implementation,
  commit, push or workflow dispatch performed for this request.
- Missing automatic local Claude completion ingress and no-op Stop projection are
  confirmed in source; permission/banner state on the user's Mac remains unknown.
- Production release Rust/ABI probe reproduces active-screen transport amplification:
  two40x120 panes transmit1.22/7.90/50.03MB/s for ASCII/color/Unicode synthetic
  screens with only about0.4KB/s cursor-update input. Unicode is deliberately
  run-heavy; no Swift/AppKit/SSH/actual Claude runtime is part of this probe.
- Exact attribution of the physical Mac31.5% CPU is explicitly the first native
  profiling gate. Historical idle-only evidence is not sufficient for that claim.
- Independent implementation phases remain unstarted, including local and remote
  Claude turn completion, native damage/caches, frame deltas and artifact delivery.
