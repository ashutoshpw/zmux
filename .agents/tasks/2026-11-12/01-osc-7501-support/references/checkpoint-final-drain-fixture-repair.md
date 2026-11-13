# Final-drain checkpoint fixture repair

**Date:** 2026-11-13
**Scope:** `crates/session-runtime/src/lib.rs` private `checkpoint_tests` and this
reference only. No production code, task status, workflow, artifact, commit, push,
or application changes.

## Diagnosis

At source `c346051`, the final-drain fixture held `Worker::execute()` while the
PTY child wrote its prompt and working report, then let the child exit before
the worker read the master PTY. The hosted universal macOS run
[`43360964642`](https://github.com/ashutoshpw/zmux/actions/runs/43360964642)
failed at `snapshot.bytes_read > 0` (line 2517). The same unmodified test passed
locally on Linux 5.15.

The source owner explains the boundary: only the PTY master read increments
`bytes_read`; that read queues actual bytes with `defer_output`; the worker later
parses the queue with `advance_deferred`. A checkpoint waits for the PTY read
drain and an empty deferred queue. Final retirement follows only after the run
loop observes child exit, EOF, and an empty deferred queue. Since the old fixture
did not let any master read happen before the child closed its slave, the
missing byte count originated before any worker read; the assertion itself ran
after a checkpoint snapshot was returned. The evidence points to a
platform-sensitive fixture assumption about unread PTY output after the last
slave closes, rather than a production checkpoint defect. The hosted failure
and Linux control do not establish a general Darwin PTY kernel rule.

## Fixture repair and contract

The final-drain child now waits on its private Unix socket after writing the
payload. While the attached child is still live, the held worker reads the exact
bytes from its real nonblocking PTY master and queues those raw bytes through its
existing deferred-output buffer. The fixture compares the captured bytes with
the child payload; it does not inject parser output, expected records, or an
acknowledgement.

The test then observes the attached child exit with status 0, admits the public
checkpoint, and loads it into the held worker before allowing `execute()` to
run. The checkpoint must remain unanswered until execution. The normal worker
path parses the deferred PTY bytes, reaches EOF, and services the checkpoint as
`Running` before `execute()` commits the attached-child-exit transition and
publishes the final `Exited` snapshot.

The checkpoint snapshot asserts the complete PTY byte count, prompt marker,
session epoch, command epoch 1, live `final` working record, and its parsed
report transition. The subsequent snapshot asserts exit status 0, the same
byte count and epochs, no live records, and the `ChildExit` transition at
command epoch 1. The live-child preexisting-output case remains in place, and
the existing `bytes_read > 0` assertion is retained.

## Test-authoring gate

1. **Observable contract:** after attached-child exit, a checkpoint still
   returns the parsed pre-retirement prompt/status and full PTY read count before
   the final snapshot clears the live record and records `ChildExit`.
2. **Credible regression:** servicing the checkpoint before deferred PTY bytes
   are parsed would omit the prompt and working record; publishing retirement
   first would return `Exited` or lose the pre-retirement state. The previous
   fixture also failed on the hosted macOS run because it left all bytes unread
   until after child exit.
3. **Existing coverage gap:** the live-child checkpoint already covers
   preexisting real-PTY bytes, while the former final-drain case depended on
   post-exit kernel buffering. Neither established the checkpoint ordering after
   observed child exit with actual bytes already retained by the Session owner.
4. **Production seam:** none added. This private owner fixture reads actual PTY
   bytes and uses the existing deferred buffer; the production worker still
   parses, checkpoints, and retires them.

## Validation

- Before repair, the final-drain test passed on this Linux 5.15 host despite the
  hosted macOS failure above:
  `cargo test --locked --offline -p zmux-session-runtime --lib
  checkpoint_tests::checkpoint_during_final_drain_keeps_pre_retirement_state --
  --nocapture`: **1 passed**.
- After repair, `cargo test --locked --offline -p zmux-session-runtime --lib
  checkpoint_tests -- --nocapture`: **2 passed**.
- Session owner targets:
  `RUST_TEST_THREADS=2 cargo test --locked --offline -p zmux-session-runtime
  --lib --test lifecycle --test program_status --test shell_integration --test
  child`: **41 passed, 2 ignored**.
- `cargo fmt --check -p zmux-session-runtime`,
  `cargo clippy --locked --offline -p zmux-session-runtime --all-targets -- -D
  warnings`, and `git diff --check -- crates/session-runtime/src/lib.rs`:
  **passed**.
- The local host is Linux, so no post-repair Darwin execution was available in
  this bounded pass. The previous hosted macOS failure is red evidence for the
  old fixture; this pass did not dispatch or wait for a workflow. Full
  `scripts/check` was not run.

## Proof limits

The fixture proves bytes were read from the actual PTY before child exit and
then processed by the production deferred parser, checkpoint, and retirement
paths. It does not claim that all bytes written after a drain observation are
included, nor that one hosted failure characterizes every Darwin PTY/kernel
configuration.

## Parent review follow-up

Independent review accepted the fixture ordering. Parent removed the P3 private
`load_input` dirty-result assertion: the scheduling gate now carries only unit,
while the public no-early-answer, parsed snapshot and retirement assertions remain.
This avoids coupling the regression to internal publication bookkeeping.
