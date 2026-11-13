# Ghostty linked Session ordering repair

**Date:** 2026-11-13
**Diagnosis:** [ghostty-session-linkage-diagnosis.md](ghostty-session-linkage-diagnosis.md)
**Hosted red control:** linkage run `43335306931`, source `9048d943eea63ff18218f0ba1b0dbfe158810ccf`

## Repair

The Session worker now publishes deferred PTY read progress through the existing
`Snapshot.bytes_read` field at the existing 16 ms snapshot cadence. This path is
active only while an output buffer has an ordering deadline behind earlier
input. It copies the last committed viewport and preserves snapshot state,
exit code, and failure. It does not refresh the terminal, advance the deferred
queue, write a causal reply, or change ordering/deadline behavior.

`bytes_read` counts bytes physically obtained from the PTY master. It is not a
parser cursor and does not mean the terminal engine has consumed or committed
those bytes. During backpressure it may lead the viewport, modes, and Program
Status snapshot. The UI can use it as a read-progress/control statistic, while
the viewport and status remain the last committed terminal state. Publishing
progress requires a copy of that committed viewport, so it is rate-limited to
the existing frame cadence rather than performed per PTY read. The copy cost
still scales with the current viewport size.

The real-PTY suffix fixture now waits until the complete query payload has been
physically read while requiring `State::Running`. It checks that viewport
generation, size, contents, title, modes, Program Status, and Session state
remain at the preceding committed snapshot. Its child expects every admitted
prefix chunk, including completed receipts and still-pending receipts, before
the query reply. The deadline fixture uses the same full-query barrier and
expects the deferred PTY ordering deadline, which is the boundary reached
before a causal reply exists. The close case still closes while output is
deferred, checks the resulting closed state and input accounting, and rejects
replay.

## Test authoring audit

- **Protected behavior:** the public snapshot reports physical PTY read progress
  while preserving committed terminal publication; queued input, query reply,
  and post-query suffix remain in their required order.
- **Credible regressions:** deferred reads never become observable before the
  ordering deadline, progress publication accidentally refreshes the viewport,
  the suffix fixture queues input before the full query is read, or the expected
  prefix omits admitted chunks not yet represented by completed receipts.
- **Why existing coverage is insufficient:** the prior real-PTY fixture waited
  on `bytes_read` but only observed it after frame commit, and expected one
  chunk instead of all admitted chunks. The hosted red control showed both
  failures. Existing tests cover adjacent reply, resize, and no-replay behavior,
  but do not cover a progress barrier under a blocked PTY write.
- **Production seam:** no new test-only seam was added. `Snapshot.bytes_read`
  already exists as production control statistics; the regression exercises it
  through a real PTY and the public Session snapshot API.

## Red-control evidence and limits

The hosted ARM64 log
`/tmp/zmux-osc7501-linkage-43335306931/execute-session-arm64.log` records both
pre-repair failures. The suffix case reached `Closed` after its read-progress
wait failed; the deadline case observed `causal PTY ordering deadline` while
asserting `terminal causal reply deadline`. The retained capacity probe records
1022 accepted bytes from a 4096-byte write before `WouldBlock`, explaining why
zero completed 4096-byte receipts did not establish zero physical acceptance.

No local macOS host is available. The linked Ghostty tests and linked acceptance
cannot be reproduced on this Linux host from that Darwin receipt. Local default
Session/PTY tests, linked-feature source compilation if available, formatting,
and strict scoped Clippy results are recorded below after execution. A source
compile is not linked Ghostty execution or linkage acceptance.

## Local validation

- `CARGO_TARGET_DIR=/tmp/zmux-osc7501-session-linkage-repair-target
  RUST_TEST_THREADS=2 cargo test --locked --offline -p zmux-session-runtime
  --tests`: **44 passed, 0 failed, 2 ignored**. The `engine` target selected
  zero tests because the optional `ghostty` feature is disabled; the default
  real-PTY, lifecycle, Program Status, shell, shutdown, and child tests ran.
- `cargo fmt --check -p zmux-session-runtime`, `rustfmt --check --edition 2024`
  on the two owned Rust files, and `git diff --check`: passed.
- `CARGO_TARGET_DIR=/tmp/zmux-osc7501-session-linkage-repair-target cargo clippy
  --locked --offline -p zmux-session-runtime --all-targets -- -D warnings`:
  passed.
- Attempted the linked feature source check with the accepted `ghostty-7501-v1`
  artifact root for `aarch64-apple-darwin`. Artifact acceptance succeeded, then
  `zmux-ghostty-vt`'s C shim build stopped before Session/test Rust compilation:
  local Ubuntu Clang 14 reported `-arch arm64` and
  `-mmacosx-version-min=14.0` as unused command-line arguments under `-Werror`.
  No linked-feature Rust source compile, final C link, or Ghostty test execution
  is claimed on this Linux host.
