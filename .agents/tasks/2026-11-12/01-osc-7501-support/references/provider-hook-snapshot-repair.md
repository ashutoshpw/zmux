# Provider-hook bounded Desktop snapshot reads

**Date:** 2026-11-13
**Scope:** Test-only snapshot handling in `apps/cli/tests/provider_hooks.rs`.
No production, FFI, shared helper API, task status, or acceptance criteria changed.

## Hosted evidence

Foundation run `43360964670`, job `131674106406`, checked out source
`c34605102c19380315c3b262686d26f7d57c6c1b`. The first provider-hooks failure
was `automatic_local_opencode_tracks_exact_root_and_preserves_user_configuration`
unwrapping `Err(Busy)` in `wait_local_provider` at line 593. That panic poisoned
the test's shared fixture lock, causing 11 later `PoisonError` failures; the
suite reported 3 passed, 12 failed, and 2 ignored. The full log is retained at
`/tmp/zmux-osc7501-final-c346051/foundation-macos-job-131674106406-full.log`.

This is evidence that a fixture snapshot read encountered legitimate
publication contention. It does not establish a failure in OpenCode setup,
provider behavior, or production code.

## Repair

The fixture imports the existing
`crates/desktop-runtime/tests/support/desktop_snapshot.rs` reader. Predicate
loops pass their existing absolute deadline to `read_until`; they do not renew
the timeout, resubmit commands, or change their predicates. One-shot snapshot
and count assertions use the helper's bounded five-second `read`. Terminal
diagnostics no longer treat `Busy` as missing output, and single-observation
alert counts cannot report zero merely because publication is in progress.

All existing assertions and test cases remain. No new regression test or
production seam was added. The two intentionally ignored child tests remain
ignored.

## Validation

At local Linux worktree source `c34605102c19380315c3b262686d26f7d57c6c1b`:

- `cargo test --locked --offline -p zmux --test provider_hooks` — **15 passed,
  0 failed, 2 ignored**.
- `cargo fmt --all -- --check` — passed.
- `cargo clippy --locked --offline -p zmux --test provider_hooks -- -D warnings`
  — passed.
- `git diff --check` — passed.

This local run does not establish a post-fix macOS Foundation result, aggregate
`scripts/check` result, or native/app acceptance. Those checks remain with the
parent task owner. No hosted workflow, build, commit, push, or app operation was
performed in this slice.
