# Provider checkpoint PTY drain repair

**Date:** 2026-11-12
**Scope:** `crates/session-runtime/src/lib.rs` and its private owner regression; no
desktop, remote, shell, dependency, artifact, commit, push, workflow, or app changes.

## Cause

`load_input()` handled `Input::ProgramStatusCheckpoint` before the loop's next
PTY read and immediately refreshed/published the terminal. A checkpoint could
therefore return command epoch A while command B's OSC 133 prompt bytes were
already readable on the PTY. The existing Desktop route test covered a stale
Desktop cache after the Session had published B; it did not cover this unread
PTY interval.

## Repair

The worker now retains a checkpoint as an ordered barrier in its existing loop.
While that barrier is pending, later queued input stays queued. The ordinary
nonblocking PTY read path continues in its existing bounded quanta. The worker
only publishes after a `WouldBlock` (or final EOF), and after deferred output,
pending PTY writes, and causal replies are committed. Reads, deferred-output
commit, and PTY writes invalidate an earlier drain observation, so a prompt or
reply arriving between parsing and checkpoint service causes another read pass.
Cancellation, deadlines, shutdown completion, final drain, and frame-publication
coalescing remain on the existing worker paths. No second parser, blocking UI
wait, polling sleep, or production test seam was added.

## Regression and negative control

`checkpoint_tests::checkpoint_waits_for_preexisting_prompt_bytes_on_real_pty`
uses `Worker::start` on its production worker thread and a real Python PTY child.
The child writes OSC 133 A plus `B_PROMPT_READY`, then signals the test over a
Unix socket. The worker remains gated after startup until the public Session
checkpoint request is admitted. This models the possible production scheduler
interleaving after the startup acknowledgement and before the worker's first
loop iteration. The returned snapshot must include the marker, PTY bytes, and
command epoch 1.

For the red control, `process_input` was temporarily restored to the former
immediate-publication path and the same test failed at `snapshot.bytes_read > 0`
with the child's write already confirmed. That temporary change was restored;
the regression then passed against the drain barrier.

## Validation

- Red control: failed for the intended stale-snapshot condition (`bytes_read`
  was 0 before the worker read the already-written prompt).
- Focused regression after repair:
  `cargo test --locked --offline -p zmux-session-runtime --lib
  checkpoint_tests::checkpoint_waits_for_preexisting_prompt_bytes_on_real_pty`
  — **1 passed**.
- Focused Session owner suite:
  `RUST_TEST_THREADS=2 cargo test --locked --offline -p zmux-session-runtime
  --lib --test lifecycle --test program_status --test shell_integration --test child`
  — **40 passed, 2 ignored**.
- Strict library Clippy:
  `cargo clippy --locked --offline -p zmux-session-runtime --lib -- -D warnings`
  — **passed**.
- Scoped formatting: `cargo fmt --check -p zmux-session-runtime` — **passed**.
- `git diff --check -- crates/session-runtime/src/lib.rs` — **passed**.

The full repository check and aggregate acceptance were not run; they remain
with the parent. The new PTY test proves the Session owner ordering for the
forced real scheduling interleaving; it does not claim that every later byte
from a continuously writing child is included after the observed `WouldBlock`.
