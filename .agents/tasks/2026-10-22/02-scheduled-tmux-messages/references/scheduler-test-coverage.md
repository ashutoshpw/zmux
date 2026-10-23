# Scheduler test coverage

## Discovery and authoring decisions

The existing primary owners already cover real remote dispatch/restart/receipts,
managed SSH routing, CLI request privacy and reconciliation, native create/edit/
cancel/CAS/countdown behavior, embedded context-menu routing, and actual AppKit
geometry. Keep those tests and their workflow gates. This follow-up adds distinct
cases to existing fixtures without any test-only production exports or hooks.

| Contract | Credible regression | Why current coverage misses it | Primary test owner |
| --- | --- | --- | --- |
| Two SSH surfaces to the same host have separate panels; reconnect preserves binding, undo-close creates a new source and leaves the old panel offline | Dedup by connection/host, or retarget to the replacement terminal | Current dedup checks use one SSH source and never reopen that source | `workspace-core/tests/remote.rs`, real actions and snapshots |
| Reopening an existing panel works at tab capacity; a new panel fails atomically until space is freed | Capacity checked before dedup, or selection/topology mutated before rejection | Existing lifecycle cases never fill the pane | Same Rust action owner, shared browser-tab fixture actions |
| An offline panel restores without a PTY or a substituted source | Restore drops an orphan or creates a local terminal | Existing orphan test validates but does not prepare the restored store | Extend the existing restore lifecycle case |
| Compound relative create/edit reaches the store as seconds; message-only preserves exact deadline and time-only preserves message | CLI drops a duration component, synthesizes a deadline on message-only edit, or clears omitted message | Existing CLI journey uses single units and a combined patch; native parsing is a separate entrypoint | Extend `apps/cli/tests/schedule_success.rs`, actual CLI with real scheduler store and isolated tmux |
| Relative list shows an empty pending queue after cancellation without changing default JSON | Renderer assumes a populated array or cancellation still appears as pending | Existing relative CLI case has one pending job | Same real-store journey |
| `--relative` is list-only; invalid durations fail before transport/journal writes | Accept flag on mutation or silently coerce malformed/out-of-range durations | Existing flag test only invokes `targets`; native grammar checks cannot guard CLI parsing | Table-driven public CLI test in `apps/cli/tests/schedule.rs` |

These are added coverage for existing behavior, rather than repairs to an observed
product defect. No production seam is needed. Tests assert action results, wire/
store data or public output, not source spelling or copied implementations.

## Validation

- Focused `cargo test --locked -p zmux-workspace-core --test remote` passed
  all six cases. `cargo test --locked -p zmux --test schedule --test schedule_success`
  passed all nine grammar/privacy/reconciliation cases; the extended real-store
  journey passed on its focused rerun after using the actual tagged deadline schema
  in its independent expected wire values.
- Three deliberate fault controls failed at the intended assertion: ignoring source
  identity selected the wrong binding; checking capacity before dedup rejected a
  live panel; replacing duration accumulation with the last unit stored 1800 seconds
  instead of 5400. Logs: `/tmp/zmux-scheduler-tests-mutation-same-host-dedup.log`,
  `/tmp/zmux-scheduler-tests-mutation-capacity-before-dedup.log`, and
  `/tmp/zmux-scheduler-tests-mutation-compound-duration-last-unit.log`.
  Production files were restored byte-for-byte before the full gate.
- Full `RUST_TEST_THREADS=2 scripts/check` passed with exit 0 on the final test
  source: all workspace tests (including six core remote cases, nine CLI schedule
  cases, the expanded real-store CLI journey, real managed SSH installation/RPC,
  and eight daemon integration cases), strict Clippy, formatting, license inventory
  and both C ABI executables. Log: `/tmp/zmux-scheduler-coverage-check.log`.
- Personal read-only post-commit review of
  `b5f2ac3b1da0edb820f5978826324ac44120a34d` found no actionable defects.
  Checked the exact committed diff, production action/descriptor contracts,
  source/config preconditions, real-store deadline/patch/receipt assertions,
  request counts/cleanup, CLI error paths and existing CI discovery. No delegation,
  source-grep tests, copied implementations, mocked scheduler acknowledgements or
  test-only production exports were introduced.
- No native source or packaged acceptance code changed; existing accepted native
  results remain tied to source `93f36fc` (14 packaged scheduler checks and ten
  actual AppKit queue resize cases). Both remain wired into macOS CI. Linux has no
  local AppKit toolchain; do not present the portable run as new native UI proof.

## Repeatable commands

```sh
cargo test --locked -p zmux-workspace-core --test remote
cargo test --locked -p zmux --test schedule --test schedule_success
RUST_TEST_THREADS=2 scripts/check
```

The CLI success case requires `tmux`; both Foundation CI hosts install it. The test
uses a private tmux socket and a real scheduler store, without dispatching messages
to any user session. For native geometry on macOS run
`scripts/check-scheduler-queue-layout`; packaged integration remains owned by
`scripts/check-macos-app` and its configured isolated SSH fixture.
