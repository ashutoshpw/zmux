# Agent lifecycle bounded snapshot fixture repair

**Date:** 2026-11-13

**Scope:** Test-only snapshot acquisition in
`crates/desktop-runtime/tests/agent_lifecycle.rs`. The existing OSC task status
and plan were not changed.

## Hosted failure

Universal development run `43395036940` checked source
`0742e26029a872449518a23eb5c5d9f30f7e3d0a` and failed `scripts/check` in
`generic_provider_observations_are_local_control_actions_with_explicit_ends`:
`desktop.snapshot().unwrap()` returned `Err(Busy)` at line 805. The complete
failed job log is
`/tmp/zmux-osc7501-final-0742e26/universal-failed-job-full.log`.

This establishes a transient nonblocking snapshot-read failure in the test
fixture. It does not show that the lifecycle behavior or its production owner
failed. The universal app build and its later native acceptance stages were
skipped. Earlier workspace-geometry acceptance remains unverified.

## Repair

The test imports the existing
`crates/desktop-runtime/tests/support/desktop_snapshot.rs` helper through the
`desktop_runtime` crate alias it expects. Every snapshot acquisition in this
file now uses `read` for one-shot observations or `read_until` for the live
predicate loop. That loop passes its original absolute eight-second deadline on
each read and retains its existing predicate and timeout. The helper retries
only `DesktopError::Busy` and fails on other errors.

No test case, assertion, action, identity, generation, alert check, or ignored
annotation was removed or changed. No fixture, helper seam, command replay,
production code, or shared test support was added or changed.

## Test authoring gate

1. **Observable contract:** Existing tests assert provider lifecycle ownership,
   transitions, generation fencing, and alert history through Desktop
   snapshots. This repair preserves those assertions and makes snapshot reads
   tolerate the documented nonblocking `Busy` result.
2. **Credible regression:** Snapshot acquisition during concurrent publication
   can return `Busy`; a direct unwrap panics, while `if let Ok` can mask other
   errors until a generic timeout. The existing helper retries only `Busy`
   within a fixed deadline and reports other errors immediately.
3. **Coverage owner:** The current lifecycle tests already own these observable
   provider contracts. The same shared test helper is used by
   `program_status.rs` and `provider_hooks.rs`; a duplicate Busy-forcing test or
   another lifecycle case would not add an independent contract.
4. **Production seam:** None is needed or added. The existing test-only helper
   is reused without changing production APIs.

## Local validation

At the shared checkout:

- `cargo test --locked --offline -p zmux-desktop-runtime --test agent_lifecycle`
  — **3 passed, 0 failed, 0 ignored**.
- `cargo fmt --all -- --check` — passed.
- `cargo clippy --locked --offline -p zmux-desktop-runtime --test agent_lifecycle -- -D warnings`
  — passed.
- `git diff --check` — passed.

This focused local run does not establish a refreshed hosted `scripts/check`,
the universal app build, or native/app acceptance. Those remain with the parent
task owner. No commit, push, workflow, or app operation was performed.
