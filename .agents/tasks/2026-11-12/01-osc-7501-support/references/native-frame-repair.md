# Native frame delta test repair

## Finding

The integrated `scripts/check` run stopped with exit 101 after 465 passed, 2 failed and 5 ignored. The first failure was `native_poll_patches_only_changed_rows_preserves_unicode_and_recovers_unknown_baselines`; the sibling test then failed because its shared test mutex had been poisoned.

A bounded diagnostic reproduction placed the timeout at `row patch after input`. The test held revision 12 and repeatedly called the real `zmux_desktop_poll_frames` ABI with that same cursor, discarding each response until `changed_rows` became nonempty. The observed responses were:

- Revision 13, `base_revision=12`: both terminal snapshots were unchanged.
- Revision 14, `base_revision=12`: session 3 was running at viewport generation 117, but the response had no nonempty changed-row patch, so the wait discarded it.
- Revision 15, `base_revision=null`: session 3 had generation 118 and `μ🙂NEXT`; the sibling had generation 74 and `OTHER_READY`.

The bridge retains two baselines. After successfully returning revision 14, its history contains revisions 13 and 14, so revision 12 is evicted. A later poll with cursor 12 correctly returns a full recovery baseline. The test was not advancing its cursor or applying returned deltas, so its repeated calls exhausted the intentionally bounded baseline history before accepting the target patch.

The input path stages a scroll-to-bottom before writing user bytes (`Input::Bytes` calls `terminal.scroll(0)`); the default engine stages that scroll and the worker publishes committed viewport state at the frame cadence. The observed generation step before the shell output is consistent with that path, followed by the output generation. No production defect was demonstrated: the shell produced the expected Unicode output, and the delta ABI followed its documented unknown-baseline recovery behavior.

After the mutex cascade was removed, the sibling link-metadata test also exposed the same stale-cursor pattern in its resize, replacement and clear waits. Its clear wait additionally treated the delta's `contents` replacement span as the complete viewport; the observed span was `NATIVE_LINKS_CLEAR`, while the synchronized full snapshot contained `NATIVE_LINKS_CLEARED`. The metadata assertions themselves remain on the delta packet.

## Repair

`crates/native-bridge/tests/native_frames.rs` now records the wait stage and a bounded diagnostic of recent distinct revisions, frame state, viewport generation and up to 48 content characters. Its shared helper waits for the complete target state through `zmux_desktop_snapshot`, which does not alter native delta history, then takes the next successful delta poll from the original revision. This is used for the changed-row output, resize, link replacement and clear stages. Each snapshot and delta wait shares the original eight-second deadline. Full contents predicates are evaluated against full snapshots; delta metadata remains checked on the actual delta packet.

All original contract assertions remain: the delta base equals the recorded baseline revision, the other pane is unchanged, only row 0 changes, other row slots are empty, UTF-8 replacement boundaries are valid, reconstructed text starts with `μ🙂NEXT`, unknown revisions recover as a full baseline, and the full snapshot API remains independent.

## Evidence

Before the synchronization change, the diagnostic run failed at the named wait and reported revisions 13, 14 and 15 above. The first test and then the final full `native_frames` target passed after correcting the cursor handling; the latter also passed the previously mutex-poisoned link-metadata test. This reproduces the failure and shows both ABI tests green without changing the native frame owner.

Focused checks passed:

- `cargo test --locked --offline -p zmux-native-bridge --test native_frames`: 2 passed.
- `cargo test --locked --offline -p zmux-terminal-adapter --test program_status --test staged_engine`: 7 passed.
- `cargo test --locked --offline -p zmux-session-runtime --test program_status`: 5 passed, including default real-PTY cases.
- `cargo clippy --locked --offline -p zmux-native-bridge -p zmux-terminal-adapter --all-targets -- -D warnings` and `cargo clippy --locked --offline -p zmux-session-runtime --lib -- -D warnings`: passed.
- `rustfmt --edition 2024 --check crates/native-bridge/tests/native_frames.rs` and scoped `git diff --check`: passed.

## Scope and limits

Only the native frame integration test and this task evidence changed; no terminal-adapter, session-runtime or native-bridge production source was modified. A broader all-target Clippy attempt stopped at the unrelated `clippy::ptr_arg` warning in concurrent, untracked `crates/session-runtime/tests/program_status.rs:83` (`&PathBuf` instead of `&Path`); that file was left untouched. The complete `scripts/check` was not rerun as directed; its original failure remains the aggregate result until the parent runs the planned full check. Native/macOS, hosted artifact and packaged-app gates were not part of this bounded remediation. Test output also included shell-integration directory-retained cleanup warnings; they did not coincide with the delta timeout and were not investigated here.
