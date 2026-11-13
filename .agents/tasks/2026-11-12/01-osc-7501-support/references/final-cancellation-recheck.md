# Final cancellation recheck

**Date:** 2026-11-13 UTC
**Baseline:** `be8ec912a3b745c328f66fd196d19ddc59920845`
**Scope:** Read-only review of the uncommitted transfer-owner, SSH integration, and OpenCode fixture changes against the baseline. No source or index edits were made.

## Verdict

The changes repair the observed late-worker `Failed` versus cancelled-delivery publication race and the OpenCode diagnostic publication wait. Focused owner, SSH, and OpenCode checks pass. One P2 state-ordering finding remains: close cancellation can also overwrite an already-terminal worker result, including a completed upload or a failure that preceded the owner’s cancellation decision. The scoped transfer change is not fully accepted until that ordering is resolved or its public-state policy is clarified and covered.

## Finding

### P2 - Close cancellation overwrites already-terminal upload state

At [transfers.rs](../../../../../crates/desktop-runtime/src/transfers.rs:180), `poll_transfers` reads whether the worker has finished, records close cancellation while `delivery` is still pending, then reads the worker snapshot. At [the reconciliation helper](../../../../../crates/desktop-runtime/src/transfers.rs:238), any snapshot state is rewritten to `Cancelled` whenever delivery is cancelled.

Reproducible owner interleaving:

1. The worker has already published `Failed` or `Completed` and finished, but `delivery` is still `pending` because the owner has not polled that terminal snapshot yet.
2. The originating surface closes before the next owner poll.
3. `poll_transfers` observes the closed origin, marks delivery cancelled, and only then consumes the terminal worker snapshot.
4. `publish_upload_snapshot` changes its `Failed` or `Completed` state to `Cancelled`.

For a preexisting failure, the diagnostic `error` and uncertainty fields remain present, but the public state no longer says the worker failed before cancellation. For an already-completed upload, the acknowledged `remote_paths`, byte counts, and non-uncertain result remain present, but the public state no longer says the remote upload completed. `delivery=cancelled` correctly prevents insertion into a closed terminal. The source lease flag still comes from `job.is_finished()` and is not released early. The issue is state precedence, not loss of the retained path or evidence explaining the failure.

The new owner regression at [transfers.rs](../../../../../crates/desktop-runtime/src/transfers.rs:285) covers a `Failed` snapshot after delivery is already cancelled, including retained bytes, uncertainty, paths, and error. It does not distinguish a late worker failure from a terminal failure that predates the cancellation decision, and it does not exercise an already-completed snapshot. The real SSH close case at [ssh.rs](../../../../../apps/cli/tests/ssh.rs:1137) proves bytes were sent before close and waits for both cancellation and source release; its stalled consumer intentionally does not cover the completed/ACKed close interleaving. The existing explicit-cancel-after-ACK case still checks that acknowledged paths survive.

To close this finding, cover both orderings at the production owner: preserve an already-terminal worker result when close is observed, while still publishing cancellation for an active transfer whose later worker teardown fails. Retain the real SSH close/source-release case and the existing ACK path assertion. Do not infer the cause of the hosted SSH `Broken pipe` from this state-publication repair.

## Other reviewed changes

- The reconciliation helper is a production helper called by `poll_transfers`, not a test-only seam. The deterministic owner test and SSH integration test protect distinct contracts and are not redundant: one pins state precedence and retained diagnostics; the other exercises the real remote consumer, close, byte accounting, uncertainty, and source release.
- The helper copies the worker snapshot fields and changes only `state`; it adds no retry or wait. Error changes participate in the publication `changed` check. The worker appends acknowledged paths, so length-based change detection still observes path additions.
- The OpenCode test now waits for the complete current `run_standalone_help_preflight` timing diagnostic and fallback marker within the original eight-second deadline after the calls-file marker. PID `ESRCH`, the 1,900-2,700 ms timing bound, one fallback call, lifecycle count, and configuration/plugin preservation assertions remain. The wait addresses the independent file-versus-PTY publication ordering; no production behavior or timeout was changed.
- The uncommitted diff does not touch Ghostty linkage or linked Session owners. The exact-`be8ec91` linkage receipt recorded in [hosted-final-be8ec91.md](hosted-final-be8ec91.md) reports ARM64 engine 4/4, linked Program Status 1/1, Session 4/4, linked real-PTY Session 1/1, plus successful ARM64 and x86_64 compile/link phases. This recheck relies on that recorded evidence and did not rerun native linkage.

## Focused validation

- `cargo test --locked --offline -p zmux-desktop-runtime transfers::tests::a_late_transport_failure_does_not_replace_close_cancellation -- --exact --nocapture` - passed, 1 test.
- `cargo test --locked --offline -p zmux --test ssh ssh_cli_opens_an_actual_remote_session_and_reopen_never_replays_its_command -- --exact --nocapture` - passed, 1 test. It emitted the recorded remote lifecycle startup retry diagnostic; that message does not identify the upload failure cause.
- `cargo test --locked --offline -p zmux --test provider_hooks automatic_local_opencode_tracks_exact_root_and_preserves_user_configuration -- --exact --nocapture` - passed, 1 test.
- `git diff --check be8ec91 -- apps/cli/tests/provider_hooks.rs apps/cli/tests/ssh.rs crates/desktop-runtime/src/transfers.rs` - passed.

No `scripts/check`, hosted workflow, app build, or native run was performed. The local host is Linux, so macOS/AppKit acceptance is unavailable here. The hosted Foundation log establishes the observed upload state (`delivery=cancelled`, 16,384 bytes sent, `Broken pipe`, uncertain remote result) and terminal-close report, but not why the terminal or remote process closed or which lifecycle diagnostic caused it.
