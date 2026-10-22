# Status

- **State:** in progress
- **Updated:** 2026-10-22
- **Authorization:** The user authorized implementation and a downloadable Mac artifact. Functional packaged acceptance and artifact verification passed; visual layout remediation and a fresh packaged run remain pending.

## Current result

- Source `12fbbe905e5b6b8f571e3ddc5d90944b7cd3033f` passed all four executed CI jobs: Swift checks for both architectures, Linux scheduler helper builds, and development validation covering Rust/C ABI, universal app build, selection/browser checks, and packaged app/sidebar acceptance.
- The packaged scheduler smoke passed all 11 checks, including five Applied receipts, unknown-outcome UI, selection receipt, and dirty-refresh CAS: original version 1, remote version 2, stale save rejected with remote state unchanged, cleanup version 3.
- Artifact [12453995367](https://github.com/ashutoshpw/zmux/actions/runs/39837043831/artifacts/12453995367), `zmux-dev-universal-12fbbe9.zip`, is 24,241,882 bytes with SHA-256 `d144313e331d822a820ed144a4b7f178628a4c4c70e3fc369b168e129cb02fd8`. Root verified its checksum, build metadata and Info SHA, helper manifest/hashes/modes, static ELF, and universal macOS binaries. Receipt: `/tmp/zmux-scheduler-verified-39837043831/zmux-macos-dev-universal-12fbbe905e5b6b8f571e3ddc5d90944b7cd3033f/download-verification.json`.
- Final screenshot review exposed a usability blocker: the message scroll area is about 50 px wide and hides the body, table columns overlap at both edges, and excessive heading gaps distort the view. Keep the task in progress until the layout is fixed and a new packaged macOS acceptance run confirms the screenshots are usable.
- The packaged checker now requires the native smoke key `scheduled_messages_layout_geometry_and_visible_text` alongside the existing 11 checks, five Applied receipts, unknown-outcome gate, and dirty-refresh CAS evidence. The new gate must confirm usable message width, contained non-overlapping panes/columns, visible State column, compact top spacing, and visible actual message text; a fresh CI run is pending the native layout fix.
- Native acceptance ran in a hosted macOS job with isolated fixtures. No physical/user Mac acceptance or user/production remote service setup or scheduled job was performed. The artifact is universal and ad-hoc signed; it is not notarized.

## Notes
- The remote service/API implementation now includes the versioned scheduler contract,
  durable store and request receipts, tmux target validation and delivery, recovery,
  and the foreground `serve`/`rpc` helper. `SchedulerService::open_with_tmux` is a
  regular dependency-injected constructor used by production environment startup and
  isolated tests.
- Service tests cover create replay/conflict, show, edit/cancel versioning and receipt
  status, reserved receipt capacity, literal pane delivery, bracketed paste, and restart
  recovery. Daemon integration tests use a private isolated tmux server and helper
  process to verify overdue recovery, a future job remaining pending across restart and
  dispatching into a byte-capture pane after clients disconnect, worker saturation,
  bounded list output, and protocol-version reporting. Fixture roots are private short
  `/tmp/zsched-*` paths with macOS AF_UNIX length assertions.
- `cargo fmt -p zmux-remote-runtime -- --check` passed. The focused 100-job helper
  list-bound test, store-lock dispatch-grace test, and restored-server restart regression
  passed after the fixture wait/cleanup correction. The frozen-tree
  `RUST_TEST_THREADS=2 scripts/check` passed with exit 0: workspace formatting and
  tests (including all 8 scheduler daemon integration tests), strict workspace Clippy,
  license inventory, and native C ABI checks all passed. The inventory covered 118
  resolved packages and 83 distinct bundled notices. The macOS development workflow
  sets `RUST_TEST_THREADS=2` on its `Validate Rust core and foreign ABI` lane to match
  the bounded fixture resource usage.
- Follow-up local `RUST_TEST_THREADS=2 scripts/check` runs passed after the callback
  and native smoke actor/focus fixes; logs are at
  `/tmp/zmux-scheduler-selector-fix-check.log` and
  `/tmp/zmux-scheduler-smoke-compile-fix-check.log`.
- CLI schedule tests passed with
  `cargo test --locked -p zmux --test schedule --test schedule_success` (7 + 1 tests).
  Strict CLI Clippy and `cargo run --locked -p zmux -- commands --check` passed. This
  is included alongside the passing full-workspace check.
- The private loopback managed-SSH fixture uses its own SSH master, remote helper,
  and isolated tmux server. It covers the selected-master scheduler route and lost
  create-reply handling without a retry; this is fixture evidence, not a test against
  a user or production account.
- Python artifact-manifest checks passed 7/7, packaging-script AST checks passed, and
  the workflow YAML parse and `git diff --check` passed.
- CI run [39812944091](https://github.com/ashutoshpw/zmux/actions/runs/39812944091)
  on `origin/main` source `2f2ff352e51682a06b0f54c8a918d7a91a804603` passed
  Swift type-checking for both architectures, both native static-musl scheduler
  helper jobs, and the full macOS Rust/C ABI `scripts/check` lane at 16:31:07 UTC.
  It also built the universal app and passed terminal selection. The scheduled-
  messages smoke then failed before reaching its UI because the app-control socket
  under `dist` had mode 0755; the runtime privacy guard correctly rejected it. The
  two initial Swift type-check failures were fixed and both architecture retries pass.
- A fixture-only repair was pushed to `origin/main` as
  `e91373127aa94f18a5aaf7b38d4224ddc749d80b`: the fixture app-control socket now
  lives in its owned 0700 scheduler state directory, separate from the daemon socket,
  and validates owner, privacy, and all socket paths below the 104-byte limit. Native
  owner AST/private-path checks and diff check passed; independent client review found
  no further happy-path blocker. The full
  `RUST_TEST_THREADS=2 scripts/check` passed at
  `/tmp/zmux-scheduler-private-fixture-check.log`.
- Follow-up CI run
  [39818421129](https://github.com/ashutoshpw/zmux/actions/runs/39818421129) on the
  repaired `e91373127aa94f18a5aaf7b38d4224ddc749d80b` source passed the core and
  universal app build, then failed the
  existing terminal-selection check before reaching the scheduler scenario: phase C
  observed a 33x100 kernel size versus the 33x103 viewport while PID, nonce, and focus
  still matched. The diagnosed race is that the child sampled size before a queued
  resize and the later UI showed the new size.
- The bounded fixture-only read-only `qprobe` handshake is committed and pushed as
  `8f4e186bf0a9e84e150ecdff27d1517c81d977ae`. The commit also adds an exact embedded
  child real-PTY regression and three failure-diagnostics archive paths. Independent
  client review found no blocker; the strict post-phase guard and identity, deadline,
  and geometry checks remain intact. Linux `--check-inputs` passed with
  `pty_probe_protocol: passed`. The same checker with the baseline child recovered
  from `e91373127aa94f18a5aaf7b38d4224ddc749d80b` fails after registration because
  `probe-w-1ack` is missing, while the current child passes. The full local
  `RUST_TEST_THREADS=2 scripts/check` passed at
  `/tmp/zmux-scheduler-selection-handshake-check.log`.
- CI run
  [39824919371](https://github.com/ashutoshpw/zmux/actions/runs/39824919371) on
  `8f4e186bf0a9e84e150ecdff27d1517c81d977ae` completed with failure after Swift
  type-check, Rust/core validation, universal app build, terminal selection, and
  browser-link smoke passed. The packaged scheduled-message smoke launched but
  failed before any scheduler controls with `checks=[]` and “Scheduled messages
  smoke has no initial workspace.” The cause was confirmed: `coordinator.start`
  schedules polling and returns before its first snapshot, while the smoke captured
  the workspace immediately. The workspace-move and scheduler-action checks were
  not reached.
- The candidate universal ZIP from `8f4e186` passed structural checks of its actual
  checksum, build metadata and Info SHA, helper hashes and modes, Linux static ELF,
  and four universal Mac binaries. This validates archive contents, not scheduled-
  message UI acceptance or a final artifact from the current source.
- The follow-up fix is pushed as
  `4879579a7fbfe3a9acc416bc9998392776eb0f78`: it waits up to 30 seconds for an
  actual focused workspace member before capture, and the diagnostic checker reports
  failure before PNG validation. Independent journey review covered target
  auto-selection, route, wire format, parser, receipt, and screenshot and found no
  blocker in that reviewed path at the time. The full `RUST_TEST_THREADS=2 scripts/check`
  passed at `/tmp/zmux-scheduler-initial-snapshot-check.log`.
- CI run
  [39830501406](https://github.com/ashutoshpw/zmux/actions/runs/39830501406) for
  `4879579a7fbfe3a9acc416bc9998392776eb0f78` completed with failure. Swift checks
  for both architectures, both Linux scheduler helpers, full Rust/C ABI validation,
  universal app build, terminal selection, and browser links passed. The packaged
  scheduler smoke passed seven checks, including lost-ack reconciliation and
  message-only, time-only, and combined message/time edits. It then timed out waiting
  for cancellation to appear in remote history. This is packaged UI evidence, but
  not complete scheduler acceptance.
- The native owner froze fixes for edit-mode/reload ordering, requested-job priority,
  and dirty-editor compare-and-swap base. Independent review found no blocker. The
  native edits remain uncommitted on the current source and have not had a new
  packaged acceptance run; keep task status in progress pending that run and the
  final artifact.
- `scripts/check-macos-app` now requires
  `dirty_refresh_retains_original_version_and_rejects_concurrent_overwrite` and
  validates `cas_refresh_rejection`: original version 1, remote version 2, cleanup
  version 3, plus distinct nonempty retained and post-rejection remote messages.
  Existing required checks and all five Applied receipt gates are unchanged. Root's
  `RUST_TEST_THREADS=2 scripts/check` passed at
  `/tmp/zmux-scheduler-final-refresh-check.log`; Python AST syntax and scoped diff
  checks passed after the checker update. The macOS report assertion still needs a
  packaged run against the frozen native source.
- A conditional DST fixture edge can miss the exact transition minute for a future
  timezone; it is a low-frequency follow-up, not the current failure or a release
  blocker.
- The failure diagnostics upload now includes the already-built universal archive,
  checksum, and build metadata. This preserves the existing success artifact gate and
  does not change test ordering or acceptance policy.
- Root downloaded the same-source Linux helper artifacts and validated their manifest,
  source binding, SHA-256, and static ELF architecture: x86_64
  `27c06837aa5c8cae6b383852450a9693bbccf8cfae1057c78656c823c3545f60`, aarch64
  `fb05c1ec5b22994ccc22d2ee167f0518cac1020db405eef0d30e57c0b310835e`.
- The packaged scheduled-message UI has been exercised on source `4879579`, but the
  cancellation-history timeout leaves end-to-end acceptance incomplete. The universal
  app build passed, but no final downloadable artifact has been verified. No
  user/production remote service setup or scheduled job has been attempted. A separate
  deferred startup cleanup issue can still loop on `Desktop is closed` after an early
  bridge-create failure; it is not the current scheduler smoke failure. Three
  dependencies have no bundled top-level license text and require upstream review
  before distribution.
