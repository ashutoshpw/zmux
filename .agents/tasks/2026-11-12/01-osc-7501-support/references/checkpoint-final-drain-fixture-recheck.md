# Final-drain checkpoint fixture recheck

**Date:** 2026-11-13

**Base:** `c34605102c19380315c3b262686d26f7d57c6c1b`

**Scope:** Read-only review of `crates/session-runtime/src/lib.rs`'s
`checkpoint_tests` diff and
[`checkpoint-final-drain-fixture-repair.md`](checkpoint-final-drain-fixture-repair.md).
No production changes or other task-file updates.

## Verdict

No blocking correctness findings. The final-drain fixture establishes the
requested PTY-read, deferred-parse, checkpoint, and child-retirement ordering.
One low-severity test-maintenance note is recorded below.

## Review evidence

- The diff is confined to the private `checkpoint_tests` module
  (`crates/session-runtime/src/lib.rs:2376+`). The final-drain
  child completes its real PTY writes before sending `wrote`, then waits on the
  fixture socket. `capture_preexisting_pty_output` polls and reads the worker's
  actual PTY descriptor, checks the raw payload, accounts for the bytes, and
  queues them with the existing `Worker::defer_output` path. It does not inject
  parsed records or parser state.
- The parent confirms the child is still live before opening the exit gate, then
  observes `Exited(0)` before admitting the public `Session` checkpoint. The
  worker fixture loads that admitted request while execution remains gated and
  confirms there is no early reply. `child::observe_child(..., false)` uses
  `NOWAIT`, so this observation does not consume the worker's retained child.
- Once released, production `execute`/`run` processes the deferred bytes through
  `advance_deferred`, services the checkpoint as `Running`, and only afterward
  commits and publishes attached-child retirement. Assertions retain the
  checkpoint byte count, prompt, session and command epochs, working record and
  parsed report transition; the final snapshot checks exit code 0, exact bytes
  and epochs, cleared records, and `ChildExit`.
- The live-child sibling still exercises an actual already-readable PTY drain
  while a checkpoint is pending. Its child finishes the PTY write loop and
  signals readiness while the worker is held, then stays alive blocked on PTY
  input. The parent admits the public checkpoint before releasing `Execute`;
  the returned `bytes_read`, prompt, and parsed working state prove the worker
  drained and parsed the real PTY payload. This sibling uses the normal worker
  read path; only the final-drain case pre-reads into the deferred queue.
- The new gates keep the existing five-second bounds on parent-side startup,
  output capture, child exit, checkpoint completion, and final retirement, and
  the existing three-second shutdown bound. Worker-side fixture receives are
  released by the parent commands; on assertion unwind, channel/socket closure
  releases those waits. No production deadline or original byte/state assertion
  was weakened.

## Non-blocking note

**P3 — private dirty-result assertion** (`crates/session-runtime/src/lib.rs:2490-2494`):
at the `load_input()` fixture call, the test asserts its returned `bool` is
false. That value is an internal run-loop
dirty/publication signal, rather than the public no-early-answer contract. The
following `checkpoint.try_recv().is_none()` and returned/final snapshots carry
the observable assertions. The `bool` check could be removed or replaced with a
snapshot observation to avoid coupling this regression to a behavior-preserving
change in `load_input`'s result semantics. This does not weaken the fixture's
ordering proof.

## Validation and limits

- This review did not run tests or broad checks; the parent is running
  `scripts/check` concurrently. A scoped `git diff --check --
  crates/session-runtime/src/lib.rs` returned no whitespace errors.
- The repair note reports the focused checkpoint and session-owner tests,
  formatting, and strict Clippy as passing. Those results were not rerun here.
- No post-repair hosted macOS run was observed. The recorded hosted
  `43360964642` failure is from the old fixture at `c346051`; it does not prove
  a general Darwin PTY rule. This review establishes source-level fixture
  ordering and the recorded local proof only, not post-repair Darwin behavior.
