# Status

- **State:** in-progress
- **Updated:** 2026-10-16

## Notes
- Actions38935487196 at8167545 passes current native repaint, sidebar, grouping,
  status-menu and hidden-app recovery assertions with correlated physical cleanup.
  Packaged plain SSH delivers the expected exact-surface notification, but remote
  hook cleanup fails the existing10s deadline on macOS. Nested SSH/tmux is pending
  there because plain cleanup fails first. No repair artifact has been published.
- Confirmed cleanup owner: Bash POSIX mode defers a HUP trap while synchronously
  waiting for the interactive external shell. Direct real-PTY regression fails
  pre-fix (`/tmp/zmux-posix-hangup-red.log`). The bootstrap now runs its child with
  explicit original stdin/default signal handling and an interruptible wait
  builtin; hook cleanup no longer waits for the foreground child. Green test also
  proves Ctrl-C/input; Rust lease is deliberately retained outside the worker so
  it cannot mask shell cleanup (`/tmp/zmux-posix-hangup-green.log`).
- Full portable check passes after this repair, including strict Clippy, notices,
  real SSH transfer/reopen, public C ABI and native lease ABI
  (`/tmp/zmux-posix-owner-full-check.log`). Release plain/nested SSH both pass
  exact-surface delivery and prompt cleanup after the owner fix:
  `/tmp/zmux-posix-owner-ssh-green.log`, `/tmp/zmux-posix-owner-tmux-green.log`.
  macOS native shell lifecycle and packaged SSH/tmux reruns remain required.
- Actions38933217178 at df1f4a8 validates the intended old-renderer metadata-only
  failure, physical cleanup, both-target Swift compilation and local completion.
  Current renderer passes the new damage assertions and proceeds through grouping,
  attention and menu mutations. A later hidden-app status-menu restore fails with
  visible=true but active=false/key=-1. No product artifact was uploaded.
- Reveal now retains the pre-unhide hidden/inactive state when deciding whether
  identity-bound Launch Services activation is needed, and renews cooperative
  activation after its asynchronous callback before making the target key. The
  unchanged hidden-app native assertion remains the regression owner; rerun pending.
- Portable recheck after fixture repair passes at df1f4a8
  (`/tmp/zmux-native-damage-layout-check.log`); this follow-up changes Swift only.
- Native Actions38931295149 at e9badee passes both-architecture Swift type checks,
  local completion and Bash/sh/zsh startup tests. Prior-renderer proof fails on
  cursor repaint rather than the intended metadata assertion, with physical app
  cleanup confirmed. Fixture now forces layout/initial display, validates visible
  geometry and observes both NSView and backing CALayer invalidation. A view-only
  flag is insufficient on layer-backed AppKit views. Native revalidation pending.
- Controlled idle Actions38931300155 at e9badee passes both120s windows with100/100
  available observations each: idle1 app CPU0.4322%, idle8 CPU0.4581%, app RSS about
 47.4/54.9MiB. Source/archive/stack hashes verified in downloaded receipts at
  `/tmp/zmux-native-idle-e9badee-38931300155/`. Workload is finite controlled PTY
  children, not actual Claude/SSH or a reproduction of the user's mostly-idle31.5%.
- Rendering71f3cf2 and nested-SSH relay e9badee pushed and remote SHA verified.
  Foundation Actions38931288016 passes on Linux and macOS. Broad native runtime
  validation and focused product artifact gates are still required.
- Added a macOS text collection command for the user: 12 process-only top samples
  followed by a 10s/1ms sample of the exact zmux-app PID, with private output.
  Physical-Mac mostly-idle attribution is still pending; hosted idle did not reproduce it.
- Authenticated nested tmux reproduced missing Stop attention before the relay
  (`/tmp/zmux-tmux-notification-red.log`). The session-owned FIFO relay passes
  plain and nested SSH journeys, with exact origin routing and cleanup, in
  `/tmp/zmux-relay-ssh-green.log` and `/tmp/zmux-relay-tmux-green.log`. One blocked
  relay process per integrated shell replaces direct writes to inner /dev/tty.
  No tmux settings or local control credentials are forwarded.
- Existing effective-OpenSSH-config test now includes RemoteCommand; prior launch
  failed with "Cannot execute command-line and remote command". A bounded worker
  probe preserves configured commands; unresolved config bypasses integration and
  leaves ordinary SSH diagnostics. Both red/green logs are retained in /tmp.
  Shared bounded runner extracted from local tmux; timeout/descendant tests pass.
- Full final portable check passes (`RUST_TEST_THREADS=2 scripts/check`,
  `/tmp/zmux-cpu-notification-final-check.log`), including the unchanged real SSH
  transfer journey and extracted bounded-runner descendant cleanup. Rendering slice
  committed as71f3cf2. Native Swift and SSH/tmux workflow gates remain pending;
  no new repair artifact has been published yet.
- Native transport now sends changed rows/UTF-8 text spans and omits unchanged
  grids; two selected-frame baselines bound memory. Swift atomically reconstructs
  coherent frames. Real ABI two-PTY/Unicode/fallback/full-reader test passes.
  Full local checks pass after retention refinement; native hosted gate remains.
- Terminal drawing caches metrics/fonts/styles and uses visual damage; unchanged
  chrome/accessibility updates are gated. Native repaint assertions and explicit
  prior-renderer regression reproduction added to the focused macOS workflow.
  Run38924449741 at53a98b5 caught a fixture reference to a local toolbar variable;
  fixed the inventory to use retained controls and exact current labels. This
  failed run is not accepted as a native build. The next workflow is required.
- Sequential diagnostic full/delta API comparison cuts colored traffic from8.003
  to0.462MB/s and Unicode stress from51.508 to1.738MB/s; these are backend payload
  observations, not a claim of physical-Mac idle CPU repair. Reference records
  exact scope, probes, library hash and pending native acceptance.
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
