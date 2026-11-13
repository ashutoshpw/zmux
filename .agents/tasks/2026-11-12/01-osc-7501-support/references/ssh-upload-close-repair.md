# SSH upload close cancellation repair

**Date:** 2026-11-13 UTC
**Baseline:** `be8ec912a3b745c328f66fd196d19ddc59920845`
**Scope:** SSH upload transfer-state publication and its loopback regression.

## Hosted failure and owner interleaving

Foundation macOS run [43349565551](https://github.com/ashutoshpw/zmux/actions/runs/43349565551), job `131637055309`, failed
`ssh_cli_opens_an_actual_remote_session_and_reopen_never_replays_its_command`
at `apps/cli/tests/ssh.rs:194`. The full log is
`/tmp/zmux-osc7501-final-be8ec91/foundation-macos-job-131637055309.log`.
Upload 12 had sent 16,384 of 8,388,608 bytes. The transfer owner reported
`delivery=cancelled` and `delivery_error=originating terminal closed`; the
upload worker reported `state=failed`, `write upload input: Broken pipe`, and
`remote_result_uncertain=true`.

Upload 12 is the final close-surface case in the SSH integration test: the test
starts an 8 MiB transfer while the fixture remote consumer is stalled, then
closes its originating surface. In `Worker::poll_transfers`, surface closure
first cancels the job and records the cancelled delivery. The upload worker
runs concurrently and can publish its pipe error before the owner reads its
final snapshot. The owner then copied that snapshot over the public transfer
state without reconciling it with its already-recorded cancellation. This
explains the contradictory `delivery=cancelled` / `state=failed` output; it
does not establish why the terminal or remote process closed.

## Repair and terminal precedence recheck

The first repair projected `Cancelled` over every worker snapshot after
delivery cancellation. Independent review found that this also masked
`Completed` or `Failed` results that the worker had already published before
the close owner polled it. The supplied red log
`/tmp/zmux-osc7501-transfer-terminal-precedence-red.log` demonstrates
`Completed -> Cancelled`.

The current owner takes a fresh `UploadJob::snapshot()` when delivery is
cancelled. If the worker is still `Preparing` or `Uploading`, it signals the
existing cancellation atomic; `retire_owned` does this before requesting the
origin PTY to close. The upload worker's existing final error path reads that
atomic and reports `Cancelled` for a teardown error after cancellation. If the
fresh snapshot is already `Completed`, `Failed`, or `Cancelled`, it does not
signal cancellation and retains that result. In every case delivery becomes
cancelled so a closed terminal cannot receive pasted paths. Snapshot-lock
contention signals cancellation without treating cached state as proof of
active work; subsequent worker snapshots remain authoritative. This uses the
existing remote-runtime API and adds no wait or retry.

Each transfer stores the originating `Slot.launch_id` alongside its session
ID. Retirement cancels only matching `(session_id, launch_id)` transfers, so
an obsolete launch completion cannot cancel work belonging to a replacement
generation with the same session ID. The source-release flag remains tied to
worker completion; progress, acknowledged paths, uncertainty, and errors still
come from the worker snapshot.

The owner regression now covers: active cancellation followed by a worker
`Cancelled` snapshot carrying the Broken pipe diagnostic and partial accounting;
preexisting `Completed` and `Failed` snapshots as separate cases; and exact
session/launch-generation matching. The original loopback test passed on the
pre-fix local source, confirming its end-to-end failure depends on scheduling;
the supplied owner red log is the deterministic terminal-precedence control.

The loopback SSH case now waits until some input bytes were sent before closing
the surface, then waits for both cancelled state and source release. It retains
the existing cancelled-state assertions and checks uncertain remote outcome,
absence of acknowledged remote paths, and a surfaced error.

## Validation

- Transfer-owner regressions: **4 passed** after repair.

  ```sh
  cargo test --locked --offline -p zmux-desktop-runtime transfers::tests -- --nocapture
  ```

- `ssh_cli_opens_an_actual_remote_session_and_reopen_never_replays_its_command`:
  **1 passed** with the loopback SSH fixture, including its close-surface upload.

  ```sh
  cargo test --locked --offline -p zmux --test ssh ssh_cli_opens_an_actual_remote_session_and_reopen_never_replays_its_command -- --exact --nocapture
  ```

- `cargo clippy --locked --offline -p zmux-desktop-runtime -p zmux --all-targets -- -D warnings`:
  passed.
- `rustfmt --check --edition 2024 crates/desktop-runtime/src/transfers.rs crates/desktop-runtime/src/lib.rs apps/cli/tests/ssh.rs`:
  passed.
- Path-scoped `git diff --check` for the three Rust files: passed.
- `scripts/check`, commits, pushes, workflow runs, and app builds were not run
  in this bounded repair.

The same hosted test log separately prints remote lifecycle cancellation
`EPERM` and a lifecycle-stream startup retry. Neither message carries the
upload ID or attributes the upload's pipe failure; this repair does not infer a
causal relationship. A post-fix loopback run also printed a lifecycle startup
retry while the SSH test passed.
