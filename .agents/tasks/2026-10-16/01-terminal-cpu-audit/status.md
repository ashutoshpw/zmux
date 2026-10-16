# Status

- **State:** in-progress
- **Updated:** 2026-10-16

## Notes
- Fast Darwin probe38943334798 narrows failure to plain SSH: local SSH client is
  gone, remote session leader is orphaned (PPID1) and waiting on foreground Bash/
  provider, both with distinct process groups. Provider INT/HUP are not ignored;
  desktop cleanup_error/last_error are null. Nested tmux succeeds. Group-HUP-only
  cleanup is insufficient; reassessed without weakening the10s deadline.
- Candidate uses interruptible owner wait plus legacy Bash3/4 signal reset by
  short-lived /usr/bin/perl that immediately execs the original shell. Modern
  Bash/dash need no helper. Private helper suppresses module autoload, restores
  user Perl environment before exec and exposes no environment values in argv.
  Direct owner/group HUP, Ctrl-C and environment preservation pass GNU Bash3.2;
  exact table fails pre-fix. Logs: `/tmp/zmux-bash32-owner-route-red.log`,
  `/tmp/zmux-bash32-reset-owner-green.log`, `/tmp/zmux-bash32-reset-env-green.log`.
  Candidate full portable recheck passes (`/tmp/zmux-legacy-wait-final-check.log`),
  as do ordinary release plain/nested SSH (`/tmp/zmux-owner-wait-{ssh,tmux}.log`).
  Darwin actual SSH probe remains required before native packaging acceptance.
- Broad Mac38941972167 passes terminal/browser render and extensive browser/native
  input/reorder acceptance, then fails existing remote clipboard directory-error
  visibility: compact header permanently hides detailLabel. Candidate reveals it
  for errors and retains compact normal chrome; native revalidation is pending.
- Actions38940969866 again passes native repaint/sidebar/status-menu and system
  Bash lifecycle tests but fails the real plain SSH disconnect10s cleanup. Group
  HUP simulation is insufficient evidence for Darwin's real SSH disconnect path.
  Reassessing actual transport before further runtime changes; no artifact handoff.
- Added a manual Rust-only macOS SSH lifecycle probe, reusing the existing ABI
  acceptance driver. On cleanup failure it retains only controlled provider
  PID/PPID/group/session/foreground-group, ignored-signal flags, bounded process
  names (no argv) and private lease filenames. No credentials/terminal contents.
  It checks both plain and nested connections even if the first fails, and fails
  the job if either fails. Local existing-driver plain/nested and syntax checks
  remain required; product sources are unchanged since362ea5c.
- Focused macOS Actions38940969866 at362ea5c passes actual system-Bash group-HUP
  cleanup and Ctrl-C, user startup and local Claude completion. Native repaint
  and packaged SSH/tmux checks remain in progress before artifact upload.
- Foundation38940970210 succeeds on Linux; macOS-latest fails allocating a fixture
  directory (EEXIST), before testing shell behavior: parallel tests shared one
  timestamp. Added a per-process atomic sequence to the private test directory
  name. No runtime source/assertion change. Full portable checks pass again
  (`/tmp/zmux-shell-fixture-allocation-check.log`). The targeted macOS run continues
  on362ea5c; the fixture-only follow-up has identical app/runtime source.
- Actions38938019009 catches Bash3.2 Ctrl-C recovery failure in the intermediate
  async-child wait repair before packaging. Reassessed with a private GNU Bash3.2
  compatibility build under /tmp (diagnosis only; no imported code/dependency).
  It reproduces inherited ignored SIGINT (`/tmp/zmux-bash32-owner-repro.log`).
- Revised lifecycle approach restores ordinary foreground interactive-shell launch.
  The already-blocked FIFO relay owns exact cleanup on SSH process-group HUP/TERM
  and normal EXIT. Real group-HUP regression fails on the prior foreground owner
  in Bash3.2 (`/tmp/zmux-bash32-group-hup-red.log`) and passes with relay cleanup,
  including Ctrl-C/input (`/tmp/zmux-bash32-group-hup-green.log`). Platform default
  Bash/sh startup and lifecycle cases also pass. Failure diagnostics now identify
  the stage and bounded controlled-PTY output. Full portable recheck passes
  (`/tmp/zmux-relay-hangup-final-check.log`). Release plain and nested SSH pass.
- Additional diagnostic substitutes only the remote bootstrap interpreter with
  the private GNU Bash3.2 build in a controlled Linux release library. Real key
  authenticated SSH and nested tmux pass notification routing and disconnect
  cleanup there (`/tmp/zmux-bash32-ssh-diagnostic-{plain,tmux}.log`). Source restored
  in finally; this mixed diagnostic library is not a product. Ordinary product
  release rebuild and plain/nested SSH tests also pass after exact source restoration
  (`/tmp/zmux-relay-hangup-product-{release,ssh,tmux}.log`). Actual Darwin package
  gates remain required before artifact handoff.
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
