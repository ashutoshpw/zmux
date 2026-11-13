# Provider-hook snapshot repair recheck

**Date:** 2026-11-13
**Scope:** Read-only source review of `apps/cli/tests/provider_hooks.rs` against
`c34605102c19380315c3b262686d26f7d57c6c1b`, with
`provider-hook-snapshot-repair.md` and the existing Desktop snapshot fixture
reader as context.

## Verdict

Accepted for this scoped source review. No concrete findings.

## Review evidence

- The source diff is limited to test snapshot acquisition and inspection in
  `apps/cli/tests/provider_hooks.rs`. It imports the existing
  `crates/desktop-runtime/tests/support/desktop_snapshot.rs` test helper; the
  helper itself and production/shared APIs are unchanged.
- Existing predicate loops pass their original absolute deadlines to
  `read_until`, including provider waits, Desktop startup/setup and alert
  observations, Codex terminal checks, and OpenCode completion/fallback checks.
  Terminal diagnostics in those loops use the same deadline. Single-observation
  snapshots and counts use the helper's bounded five-second `read`.
- The helper retries only `DesktopError::Busy`, bounds each sleep by the
  remaining duration, panics when Busy persists to the supplied deadline, and
  panics on other errors. Counts are computed only after a successful snapshot,
  so Busy cannot be converted into a zero count.
- No command submission or provider invocation changed in the diff. All test
  annotations and both ignored-test annotations remain. The diff retains the
  existing assertions; the source has the same 222 assertion-macro lines as the
  baseline. `git diff --check` is clean.

## Evidence limits

- The repair note reports `provider_hooks` at 15 passed, 0 failed, 2 ignored,
  plus formatting and scoped strict Clippy passes. Those commands were not
  rerun in this read-only review.
- This review does not establish a post-fix macOS Foundation run, aggregate
  `scripts/check`, native/app acceptance, or artifact delivery. Overall OSC
  task acceptance remains with the parent task owner.
