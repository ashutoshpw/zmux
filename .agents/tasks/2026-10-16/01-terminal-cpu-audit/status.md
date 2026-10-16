# Status

- **State:** in-progress
- **Updated:** 2026-10-16

## Notes
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
