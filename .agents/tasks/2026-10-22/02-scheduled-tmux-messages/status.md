# Status

- **State:** in progress
- **Updated:** 2026-10-22
- **Authorization:** The user authorized implementation and a downloadable Mac artifact. Service, CLI, and AppKit source changes are frozen for final validation; packaged Mac delivery and acceptance remain pending.

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
- Native schedule UI and its lost-reply smoke fixture are present in source. The packaged
  Mac smoke has not run, and no user/production remote service setup or scheduled job
  has been attempted. The packaged Mac build, scheduled-messages smoke, downloadable
  artifact, and native acceptance remain pending. Three dependencies have no bundled
  top-level license text and still require upstream review before distribution.
