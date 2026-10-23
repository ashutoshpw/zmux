# Status

- **State:** done
- **Updated:** 2026-10-23
- **Authorization:** The user authorized implementation, relative times, an embedded Schedules tab, main delivery, post-commit self-review and a downloadable Mac build. This extension is complete; earlier scheduler follow-ups remain tracked.

## Accepted relative scheduling and embedded-tab build

- Download final artifact
  [12471043165](https://github.com/ashutoshpw/zmux/actions/runs/39880328272/artifacts/12471043165):
  `zmux-dev-universal-93f36fc.zip`, 24,347,784 bytes, SHA-256
  `045ad8b1378bbaf96edfa54728cc9572b2a74283a0347d21670bd735c6321765`.
  Application/test source is `93f36fcab3ac89fa0fa32639a8b140f5bfd5066a`, pushed
  on main. The final delivery receipt changes documentation only.
- [Run 39880328272](https://github.com/ashutoshpw/zmux/actions/runs/39880328272)
  completed **successfully**: both Swift architectures, both Linux helpers, the
  full Mac Rust/C ABI gate, universal build, terminal-selection/browser links,
  packaged app/rendering/scheduler, sidebar and system-status-menu checks passed.
- All **14 packaged scheduler checks** passed: true embedded tab (no extra window),
  clicked-workspace source scope and deduplication, remote refresh, lost-ack receipt
  reconciliation, create, separate/combined edits, cancellation/history, geometry
  and visible message text, five Applied receipts, original-version CAS preservation,
  relative create/edit/countdown and exact deadline preservation on message-only edit.
  The separate fast AppKit owner check passed all **10 resize cases**, including
  overlay and legacy scrollers with a 455-point dense legacy viewport.
- Root downloaded the final artifact and verified the archive checksum, matching
  outer/bundled build metadata and Info SHA, macOS 14 minimum, all four universal
  Mac binaries, all three helper manifest hashes/modes/static Linux architectures,
  scheduler source/checks/receipts/CAS and completed selection receipt. Root visually
  inspected the final screenshot: Schedules is beside the SSH terminal in the main
  UI, message text is legible, and State/version is fully visible without overlap.
  Receipt: `/tmp/zmux-relative-tabs-delivery-39880328272/download-verification.json`.
  Fast owner evidence: `/tmp/zmux-queue-layout-green-39880328272/`.
- Build is ad-hoc signed and **not notarized**. Hosted UI/SSH/systemd fixtures do
  not establish physical-user-Mac or production remote-account acceptance. Existing
  explicit accessibility Save acceptance remains deferred by the established workflow;
  this extension does not modify file/browser Save behavior. No production service
  or scheduled message was installed/created during this extension.
- Quit the older Zmux app, extract the inner ZIP and launch the new app. Right-click
  a managed SSH workspace and choose **Schedules**, or use Workspace > Scheduled
  Messages. Both open the same bound tab. New messages support In/At with presets;
  pending rows support live relative/absolute display. CLI `schedule list --relative`
  prints a snapshot; default JSON and remote deadline authority are preserved.
- Personal read-only post-commit review is recorded in
  [relative-tabs-review.md](references/relative-tabs-review.md). Hosted verification
  found and closed the stale format assertion and native column-spacing defects.
  Earlier message-only-minute-rounding and New-from-History issues are also fixed;
  the remaining previously recorded scheduler issues were not re-audited in this slice.
  Latest full local gate passed at `/tmp/zmux-relative-tabs-spacing-check.log`.

## Implementation and validation history

Implemented relative entry (`1h30m`, presets, In/At), display-only live countdowns,
and `schedule list --relative` while preserving default JSON. Schedules is a real
Rust surface with no PTY, exact SSH source ID, deduplication and session format 11
(model 8); older sessions remain readable. Native panels are retained across tab
switches/moves, go offline if their source disappears, and never rebind by index.
Hidden SSH health is published separately without hidden terminal content.
Both main-menu and correctly scoped workspace context-menu actions share the core
action. Message-only edits preserve the remote deadline including seconds.

Focused CLI, source/restore and hidden-route checks passed in the workspace run.
The required `RUST_TEST_THREADS=2 scripts/check` passed with exit 0 at
`/tmp/zmux-relative-tabs-final-check.log`: all workspace tests, strict Clippy,
formatting, license inventory and both C ABI checks.
The first full pass found remaining format-10 expectations in the group persistence
test; those now assert 11 while retaining legacy reader coverage. Native Swift and
expanded embedded/relative UI acceptance require the hosted Mac build (no local
Swift toolchain). The previous installer run 39858685541 is now confirmed successful.
Post-commit personal `review-agent` review of `13ccf1c` found no new actionable
defects; see [relative-tabs-review.md](references/relative-tabs-review.md). A new
Mac artifact, hosted acceptance and delivery verification remain pending.

- Build run 39867091305 passed both Swift architectures, both Linux helpers,
  the full Mac Rust/C ABI gate, universal packaging and terminal-selection/link
  acceptance. It stopped in `check-native-windows` before the new scheduler UI:
  a missed fixture assertion still required session format 10. Updating that
  assertion to 11 preserves every topology, selection, todo, identity and final
  save check. Full local `scripts/check` passed again with exit 0 at
  `/tmp/zmux-relative-tabs-fixture-check.log`; Python compilation also passed.
  No application code changes are needed for this failure. Log:
  `/tmp/zmux-relative-tabs-native-failure.log`. A repository-wide version-expectation
  search found no other stale executable 10 expectations.
- Candidate 12465123275 is package-verified (source 13ccf1c, SHA-256
  `cd29a0b47b688e9020fb60e8728a3ca096698891d46a06e86a23126e3263cc74`,
  24,344,586 bytes), but scheduler UI acceptance has not run and it is not yet
  delivered as this feature's accepted build.

- Fixture repair `fa1b43b0a71546b2bd2bf3d46ac42648d680a594` is on origin/main
  with exact remote parity. Personal post-commit review found no further defect.
  Fresh run [39870897287](https://github.com/ashutoshpw/zmux/actions/runs/39870897287)
  is in progress, with both Linux helper builds passed. New accepted artifact is pending.

- Run 39870897287 passed the corrected native save/reopen persistence journey.
  Its scheduler report confirms the embedded tab, correctly scoped context menu,
  no extra window/dedup, managed SSH refresh, lost-ack reconciliation, create,
  message-only edit, time-only edit and combined edit. It then correctly failed
  the existing geometry gate: the State column extended outside the queue viewport.
  The screenshot visibly confirms a real embedded tab and clipping of State/version.
  Make the table follow its viewport width and allow all columns to resize uniformly;
  retain the strict column-visibility assertion. The repair pins document width
  in scroll-view tiling and enables uniform column autoresizing, with dimensions
  added to failure diagnostics. Full portable `scripts/check` passed again
  (`/tmp/zmux-relative-tabs-layout-check.log`). AppKit acceptance must rerun.
  Evidence: `/tmp/zmux-relative-tabs-diagnostics-39870897287/`, failure log
  `/tmp/zmux-relative-tabs-native-second-failure.log`.

- Layout repair `db18ba227063fc92378bb075e03f5d31b81ad200` is on origin/main
  with exact parity. Its personal post-commit review found no further defect.
  Run [39875315431](https://github.com/ashutoshpw/zmux/actions/runs/39875315431)
  is the current pending Mac build and native acceptance run. Do not deliver the
  earlier candidate as accepted; the retained strict geometry gate has not passed yet.

- Run 39875315431 failed the same strict column visibility check. Measurements
  prove table frame/viewport both 483 points, but the final column ends at 523.
  Stop broad build retries and use a native component-owner check first. Extract
  the real production queue/table configuration to `ScheduleQueueView.swift`,
  reused by the application and a standalone AppKit resize fixture. No copied
  layout algorithm, stub bridge or test-only production hook. The existing packaged
  check remains the native-host integration gate. The focused fixture must show
  the overflow red before repairing style/column sizing, then pass width/resizes.
  It runs before Swift checking and the costly universal build in CI. Existing
  queued smoke remains strict; no artifact is delivered as accepted yet.
  Local full verification passed at
  `/tmp/zmux-relative-tabs-queue-owner-final-check.log`; one unrelated existing
  link-test Busy transient passed unchanged on focused and full reruns.

- Fast native run 39879610961 reproduced the exact overflow against the extracted
  production owner before a full build: column widths remain [150,100,110,90],
  automatic style adds 17-point intercell spacing, and State ends at 523 versus
  a 483-point viewport. Receipt and screenshot are retained at
  `/tmp/zmux-queue-layout-red-39879610961/`; the expensive build was correctly skipped.
  Fix the production owner with explicit full-width style and zero extra spacing;
  also verify both overlay and legacy scrollers through the same resize sequence.
  Full local `scripts/check` passed with exit 0 at
  `/tmp/zmux-relative-tabs-spacing-check.log`. Native red is recorded before this fix.

- Spacing repair `93f36fcab3ac89fa0fa32639a8b140f5bfd5066a` is pushed.
  Fast native run [39880328272](https://github.com/ashutoshpw/zmux/actions/runs/39880328272)
  passed all 10 real AppKit geometry cases: overlay row count 1 and legacy row
  count 30, widths 483/470/640/470/483. At the original case, State now ends at
  483 within 483; at narrow legacy, it ends at 455 within 455. Style is explicit
  full-width and intercell spacing is (0,0). Report source matches 93f36fc.
  Evidence: `/tmp/zmux-queue-layout-green-39880328272/`, artifact 12470351277.
  Post-commit personal review found no additional defects; full app Swift checks,
  packaging and packaged scheduler acceptance remain pending in that run.

## Prior installer result

- Verified fixed candidate
  [12461339273](https://github.com/ashutoshpw/zmux/actions/runs/39858685541/artifacts/12461339273)
  contains `zmux-dev-universal-d58bf47.zip` (24,262,463 bytes), SHA-256
  `2308a7105d69dde505609f577647d5cc8525239ba8d0d929d1c25016e3760f1f`.
  Source is `d58bf473fdbbf759dd5da949389ba31135d527de`, including installer fix
  `e7e4b8e171dc7ff329c3cf6e1b4f2d7299ce9109`. Root downloaded the actual candidate,
  extracted it, and verified its sidecar checksum, matching bundled/outer build
  metadata and Info SHA, macOS 14 minimum, all four universal binaries, all three
  helper manifests/hashes/executable modes and static Linux ELF architecture.
  Receipt: `/tmp/zmux-installer-candidate-39858685541/download-verification.json`.
- The new candidate is ad-hoc signed and not notarized. The real-SSH installer
  regression, full local check, full macOS Rust/C ABI check, universal build and
  candidate upload passed. **Full CI is not established**: terminal selection,
  browser/native UI and sidebar acceptance were still in progress or pending at
  handoff. Prior seven scheduler review findings and the unconfirmed late-pipe
  observation below remain outside this repair. No user/production systemd service
  was installed; systemd lifecycle calls in the installation regression are fixtures.
- Handoff: quit the old Zmux app, extract the inner archive, launch the new app,
  and retry **Install Helper**. The original rejection happened before installer
  spawn, so that failed attempt did not install the scheduler.

- Final code/test source `d58bf473fdbbf759dd5da949389ba31135d527de` is on
  `origin/main` with exact SHA parity. Run
  [39858685541](https://github.com/ashutoshpw/zmux/actions/runs/39858685541) passed
  both Swift architecture checks, both Linux helpers and the **full macOS Rust/C
  ABI `scripts/check` gate** at 21:48:38 UTC. This includes the new real-SSH fresh
  installation regression and the repaired browser fixture. Universal packaging
  is in progress. Read-only self-review of the installer and follow-up diffs found
  no new actionable defects; prior seven scheduler findings remain outside scope.

- Final local `RUST_TEST_THREADS=2 scripts/check` passed with exit 0 after the
  wrapper-contract adjustment (`/tmp/zmux-installer-delivery-check.log`), including
  the real-SSH fresh installer, all workspace tests, strict Clippy, formatting,
  license inventory and both C ABI executables. The focused regression also passed
  (`/tmp/zmux-installer-loopback-wrapper-check.log`). Workflow YAML and preservation
  of all later acceptance steps were checked. A new Mac candidate is pending.

- Build [39856709153](https://github.com/ashutoshpw/zmux/actions/runs/39856709153)
  passed Swift and Linux helpers, and the browser wire fixture passed on macOS.
  Its new installer regression completed the install/checksum/file assertions but
  failed on the extra wrapper-execution SSH operation with `ssh_process_leaked_pipe`:
  the multiplex client exited before all output handles closed within 250 ms.
  Root cause of that late close is unconfirmed; transport watchdogs remain strict.
  Keep real managed-SSH execution for installation and its corrupted-payload case;
  execute the installed wrapper directly on the shared private loopback filesystem
  to guard its executable-file contract without repeating the independent RPC
  transport regression. Preserve every file, mode, unit and lifecycle assertion.
  Failure log: `/tmp/zmux-installer-macos-final-failure.log`.
- Retain a clearly named `zmux-macos-candidate-universal-SHA` archive immediately
  after successful packaging. All native acceptance steps and the final gated
  development-artifact upload remain in place. Candidate delivery requires exact
  source binding, archive checksum, universal app/CLI/bridge, all bundled helpers,
  and disclosure of pending or failed later gates; it does not mean full CI passed.

- Repair commit `e7e4b8e171dc7ff329c3cf6e1b4f2d7299ce9109` was pushed with
  exact remote-SHA parity. Read-only self-review using `review-agent` found no new
  defects in the installer diff; the earlier seven scheduler findings remain.
- Build run [39855041065](https://github.com/ashutoshpw/zmux/actions/runs/39855041065)
  passed both Swift architectures and both static Linux helpers, then failed before
  the app build in `browser_wire_preserves_scripts_options_and_existing_target_precedence`.
  Its capture peer accepted a nonblocking listener socket and hit macOS `WouldBlock`
  on the frame header (`apps/cli/tests/browser.rs:595`). This test file is unchanged
  by the installer commit. Make the accepted fixture stream explicitly blocking
  before applying its existing two-second read timeout; retain all framing assertions.
  Failure log: `/tmp/zmux-installer-macos-failure.log`. New native build still pending.
- The browser fixture repair passed its focused check
  (`/tmp/zmux-installer-browser-fixture-check.log`). The subsequent full check passed
  workspace tests, formatting, strict Clippy and inventory, but its C ABI caller
  intermittently failed at the unrelated terminal-sharing replacement-grant assertion
  (`abi.c:412`). No native/runtime code changed for the fixture repair. An immediate
  unchanged `scripts/check-native-bridge` rerun passed both ABI executables
  (`/tmp/zmux-installer-final-abi-check.log`). Retain this transient as a validation
  limit; do not describe that full-check invocation as exit 0.

- Fresh-install repair authorized: reproduce the user's control-character failure
  at the generated-command/managed-SSH boundary, preserve strict command validation,
  verify remote file contents and wrapper execution, then deliver a new Mac build.
  Test gate: the regression guards clean installation; embedding literal newlines
  must fail before SSH spawn. Existing RPC/UI tests preinstalled the helper, so they
  cannot catch this path. Reuse the private SSH fixture without new production seams;
  systemd lifecycle calls are isolated fixtures, not host service mutations.
- The new `fresh_scheduler_installation_writes_executable_files_through_managed_ssh`
  regression failed on the original installer with `InvalidOperationCommand`
  (`/tmp/zmux-installer-regression-red.log`). It passes after quoting each generated
  file line separately and reconstructing LF separators with remote `printf`.
  Strict SSH command validation is unchanged. The same real-loopback test checks
  default/custom paths (spaces, apostrophes, backslashes and shell metacharacters),
  corrupt-payload rejection, helper bytes, private modes, unit directives, wrapper
  execution and start/restart calls. Systemd calls are fixture-backed; this does not
  establish installation on a user's real Linux/systemd account.
- The path case exposed GNU checksum filename escaping; setup checks and installation
  now hash file contents via stdin to keep the digest independent of path spelling.
- `cargo test --locked -p zmux-desktop-runtime fresh_scheduler_installation_writes_executable_files_through_managed_ssh -- --nocapture`
  passed (`/tmp/zmux-installer-regression-final.log`), the five scoped scheduler tests
  passed, and `RUST_TEST_THREADS=2 scripts/check` exited 0
  (`/tmp/zmux-installer-full-check.log`): workspace tests, strict Clippy, formatting,
  license inventory and both native C ABI checks passed. New Mac artifact pending.
- The seven earlier self-review findings are outside this installer repair and remain
  unresolved. The prior artifact's separate sidebar notification cleanup failure
  remains a validation limit until a new native run establishes its current result.

- Source `43d5111ac7f6d237fb94431d3bb7dd96ca816e6c` passed the scheduler-specific packaged acceptance in CI run [39843250131](https://github.com/ashutoshpw/zmux/actions/runs/39843250131): both Swift architectures, Linux helpers, full Rust/C ABI validation, universal app build, terminal selection, browser checks, and packaged scheduler smoke passed. All 12 scheduler checks passed, including `scheduled_messages_layout_geometry_and_visible_text`, five Applied receipts, lost-ack/unknown-outcome behavior, selection, and dirty-refresh CAS (original version 1, remote version 2, stale save rejected without changing remote state, cleanup version 3).
- Root's white-background HTML render of the retained populated-history screenshot confirmed legible message content, a visible State column, compact heading spacing, and no pane/table overlap. Scheduler visual acceptance is complete.
- The overall CI run is **failed**, not fully green: a separate sidebar shutdown job reported all 10 sidebar checks and all 10 status assertions true, but physical cleanup was false. It reported `Notification synchronization: Native notificationEvent(_:) failed (-6)`; the cause of the cleanup failure is unconfirmed. The sidebar smoke does not open `ScheduleMessagesWindow`, and no causal link to scheduler layout was found. App-wide notification synchronization and physical cleanup remain unresolved validation limits; no remediation was performed here. The normal success-artifact upload was skipped.
- Verified downloadable diagnostic artifact [12455043815](https://github.com/ashutoshpw/zmux/actions/runs/39843250131/artifacts/12455043815) contains `zmux-dev-universal-43d5111.zip` (24,253,392 bytes; SHA-256 `b6e7735d568ccfd6e1c14ec25de72e2f909ca283b0963c9051528d4a721eeb80`). Root verified build metadata and Info SHA, all three helper manifest hashes and executable modes, static Linux ELF, universal app/CLI/bridge/Darwin helper binaries, the 12-check report, CAS, receipts, selection, and screenshot. Verification receipt: `/tmp/zmux-scheduler-diagnostics-39843250131/Zmux-native-diagnostics-43d5111ac7f6d237fb94431d3bb7dd96ca816e6c/download-verification.json`.
- The artifact is universal and ad-hoc signed, not notarized. Acceptance ran on a hosted macOS job with isolated fixtures; no physical/user Mac acceptance or user/production remote service setup or scheduled job was performed.
- Deferred follow-ups remain: a conditional DST fixture edge can miss the exact transition minute; an early bridge-create failure can still loop on `Desktop is closed`; three dependencies lack bundled top-level license text and need upstream review before distribution.

## Prior run history

The entries below document earlier attempts and are superseded by the current result above.

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
