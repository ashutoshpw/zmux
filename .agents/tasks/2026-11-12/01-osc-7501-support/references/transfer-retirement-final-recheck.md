# Transfer retirement final recheck

**Date:** 2026-11-13 UTC
**Baseline:** `be8ec912a3b745c328f66fd196d19ddc59920845`
**Scope:** Read-only review of the shared transfer-retirement diff in Desktop and its SSH loopback coverage. No source, index, or unrelated task files were edited.

## Verdict

The prior P2 terminal-precedence finding is closed. I found no remaining P1/P2 source defect in the scoped retirement/cancellation path. The repair preserves worker-published terminal outcomes, cancels delivery before a closing Session is asked to stop, and binds retirement to the upload's originating launch generation.

## Source review

- Upload admission copies `slot.launch_id` into the private `TransferSlot` ([`transfers.rs`](../../../../../crates/desktop-runtime/src/transfers.rs#L87), [`transfers.rs`](../../../../../crates/desktop-runtime/src/transfers.rs#L145)). Retirement matches both `session_id` and `launch_id`, and only visits pending deliveries ([`transfers.rs`](../../../../../crates/desktop-runtime/src/transfers.rs#L233)).
- `retire_owned` invokes origin cancellation before `Session::request_close` ([`lib.rs`](../../../../../crates/desktop-runtime/src/lib.rs#L3030)). The fresh `UploadJob::snapshot` drives the cancellation decision: `Preparing`/`Uploading` signal the worker atomic; an already `Completed`/`Failed`/`Cancelled` snapshot does not. An unavailable/busy snapshot cancels delivery and signals the atomic without using cached upload state to infer activity. Later publication copies the worker snapshot; there is no unconditional `Cancelled` projection ([`transfers.rs`](../../../../../crates/desktop-runtime/src/transfers.rs#L258)).
- A cancelled delivery bypasses the `Completed` path insertion branch, so acknowledged paths remain observable without being pasted into a terminal that is closing ([`transfers.rs`](../../../../../crates/desktop-runtime/src/transfers.rs#L182)). Snapshot publication retains bytes, totals, paths, uncertainty, and error; the source-release flag remains derived from `job.is_finished()` ([`transfers.rs`](../../../../../crates/desktop-runtime/src/transfers.rs#L163), [`transfers.rs`](../../../../../crates/desktop-runtime/src/transfers.rs#L247)). The upload worker's existing final error classification and ACK accounting in `remote-runtime` are unchanged.

### Retirement call paths and interleavings

- Surface removal and SSH reconnect emit `StopSession`; Desktop removes the old slot and passes its launch ID to `retire_owned`. Reconnect emits the old stop before starting its replacement ([`surfaces.rs`](../../../../../crates/workspace-core/src/surfaces.rs#L421), [`lib.rs`](../../../../../crates/desktop-runtime/src/lib.rs#L5064)).
- Normal, stale/obsolete, cleanup, restore, and absent-slot launch completions all route owned Sessions through `retire_owned` with the completion's ID and launch ID ([`lib.rs`](../../../../../crates/desktop-runtime/src/lib.rs#L5361)). An obsolete completion therefore cannot match a transfer admitted under a different launch generation, even when the session ID is reused.
- Shutdown first invokes the existing global `cancel_transfers`, then retires each current Session and any queued completion Sessions through `retire_owned` ([`shutdown.rs`](../../../../../crates/desktop-runtime/src/shutdown.rs#L242)). That global atomic signal is unchanged by this diff; the owner-specific path still applies exact-generation delivery cancellation and does not rewrite terminal snapshots.
- If a terminal upload snapshot is already published when close/cancel is handled, its state and diagnostic fields survive while delivery becomes cancelled. If the upload is active, delivery is cancelled first and the worker receives the cancellation signal; a later transport error remains visible with its accounting and uncertainty. If the snapshot lock is busy, cancellation is signalled and later worker truth is retained. In all close cases, cancelled delivery prevents late insertion.

## Focused validation

Fresh local execution used an isolated Cargo target directory:

- `cargo test --locked --offline --target-dir /tmp/zmux-osc7501-transfer-review-target -p zmux-desktop-runtime transfers::tests -- --nocapture` — **4 passed**: active cancellation plus diagnostic/accounting, preexisting `Completed`, preexisting `Failed`, and exact session/launch matching.
- `cargo test --locked --offline --target-dir /tmp/zmux-osc7501-transfer-review-target -p zmux --test ssh ssh_cli_opens_an_actual_remote_session_and_reopen_never_replays_its_command -- --exact --nocapture` — **1 passed**. This loopback test includes successful insertion, explicit cancel after remote ACK with the acknowledged path retained, ACK failure, explicit active cancellation, and close after bytes have been sent. The close assertion waits for cancellation and source release and checks uncertainty, no acknowledged path, and a surfaced error.

These cases protect distinct terminal-outcome and delivery policies; no test-only production seam was added. The generation regression directly exercises the production matcher, while the source call-path review confirms retirement uses it. It does not drive an obsolete completion through the full Desktop event loop.

The SSH test emitted the known remote lifecycle startup-retry diagnostic and passed; it does not attribute the upload pipe outcome to that diagnostic. No `scripts/check`, hosted workflow, app build, or native execution was run as part of this bounded review.

## Separate Ghostty linkage evidence

The retained linkage receipt and logs are for exact source `be8ec912a3b745c328f66fd196d19ddc59920845`, matching this checkout's `HEAD`. The downloaded linkage ZIP SHA-256 is `216fa1e3a07f3e6fab045f3589fa25077f1c0de037b1f1dd6819774eb4502b70`; the receipt SHA-256 is `99331b30a93d32b94abd64d8cb5bbf0445c2093de99644b452565cc00cd3b41d`. I independently checked all 229 receipt `source_sha256` entries against Git blobs at that exact commit: 229 matched, 0 mismatched (228 normalized paths, with the recorded `scripts/process_group.py` alias).

The retained receipt reports ARM64 engine 4/4, linked Program Status 1/1, Session 4/4, linked real-PTY Session 1/1, and successful ARM64/x86_64 compile-link phases. It does not claim Intel execution, AppKit linkage, or packaged-app acceptance. The current scoped diff contains no Ghostty or linkage-input paths, so this artifact evidence remains separate from the uncommitted transfer repair.

## Proof limits and parent gate

- This is source review and focused Linux execution, not macOS/AppKit or packaged native acceptance.
- The parent reported that its aggregate `scripts/check` stopped at 456 passed, 1 failed, 5 ignored on an existing `terminal_browser_links.rs:24` `Desktop::snapshot()` `Busy` fixture failure. The parent has since made a test-only bounded-wait repair and reports both focused browser-link tests pass; aggregate rerun remains pending. That separate fixture is outside this review's scope.
- The OpenCode publication-wait repair was already accepted by prior review and is not re-audited here because no transfer integration issue required it.
