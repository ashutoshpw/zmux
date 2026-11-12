# Completed repair recheck

**Date:** 2026-11-12

**Scope:** completed repairs 1, 2, 3, 4 and 5 from the review request. The active provider-epoch implementation was excluded. No source or index files were changed.

## Verdict

No runtime defect was found in the completed repair paths reviewed here. Two P3 test-quality findings remain below. This is a scoped source review, not overall integration or delivery acceptance. The linked Ghostty Session test still needs execution with an accepted fresh linked library.

## Findings

### P3: round-trip test hard-codes current bootstrap size

In crates/session-runtime/src/shell_integration.rs:496-500, the byte-for-byte compression round-trip test requires the current embedded source to exceed 128 KiB. A behavior-preserving reduction below that threshold would fail despite successful round-trip recovery and a valid, bounded SSH command. The previous 146,116-byte E2BIG case was separately reproduced, and the current owner-level test in crates/session-runtime/tests/shell_integration.rs:77-90 launches /bin/true with the actual remote command and enforces a 64 KiB transport budget. The task record also reports three passing loopback managed-SSH tests.

Keep the exact decoded-bytes equality and 48 KiB encoded-payload bound. Remove the source-size assertion and rename the test to describe round-trip behavior. No synthetic large fixture or codec refactor is needed.

### P3: shared Session test assertions do not prove all named lifecycle claims

In crates/session-runtime/tests/program_status.rs:117-161, the final write includes both FINAL-DRAIN text and state=working:id=last, but the assertions only check that the text arrived and the final record list contains finished. They can pass if the final OSC is dropped after its text bytes are drained. Assert the transition for id=last and its ChildExit retirement to prove the final OSC was parsed before retirement.

The shared close helper at lines 331-401 publishes a working record, waits until it is visible, then shuts down the idle child. Its assertions meaningfully verify State::Closed, empty records, and a TerminalClosed transition after the Ghostty engine-abort path. There is no staged or canceled frame in this fixture, so the helper name claiming closure “without publishing canceled frames” is not directly asserted. The Ghostty test still covers the metadata-clear regression; narrow that name/comment to the metadata-clear contract unless a canceled-publication scenario is intentionally added.

## Repair review

- **Compressed bootstrap:** Cargo.toml selects flate2 with default features disabled and rust_backend enabled. shell_integration.rs bounds expanded and encoded payload sizes, caches all four bootstrap variants with OnceLock, detects GNU and BSD Base64 flags, and falls back by shifting the wrapper arguments and execing the original shell argv when tools or decoding are unavailable. The sentinel preserves trailing newlines through command substitution. The decoder does not require Python and does not block on an external compressor. The task record reports the repaired command at 41,957 bytes, successful direct exec, shell and Program Status focused tests, and three loopback SSH tests. Those session/SSH receipts were not rerun in this review.
- **Linked Session PTY test:** session-runtime/tests/program_status.rs:83-162 uses feature-gated Ghostty EngineOptions and Session::spawn_with_engine. The child verifies the exact capability reply followed by Ghostty's distinct DA reply, writes a final tail containing another working status at EOF, and the assertions check final-drain retirement. The same test invokes the explicit-close fixture. The close fixture has meaningful real-PTY setup and asserts cleared committed metadata and TerminalClosed after Kill shutdown. Linked execution was not available because the task records no accepted fresh Ghostty library.
- **Ghostty close owner:** ghostty-vt/src/owner.rs:477-485 rejects a staged batch and mutates only the committed Rust ProgramStatusStore. It performs no C engine access after abort and does not publish a staged batch. No unsafe reuse path was found.
- **Malformed duplicate IDs:** program-status/src/lib.rs:749-764 skips invalid ID values before applying ID-specific caps. The P05 loop at program-status/tests/protocol.rs:545-562 checks over-total, over-segment and over-depth malformed duplicates, preserves the valid target selection, changes its state and keeps root unchanged. Existing P03 cases at lines 335-365 still reject valid oversized total, segment and depth IDs atomically. The focused P05 test passed in this review.
- **Artifact digest fixture:** tools/terminal-engine-artifacts/test_candidate_sets.py:128-176 supplies all producer recipe siblings. Lines 217-248 change well-shaped leaves under recipe, Ghostty, Zig, patch hashes and developer/toolchain fields while retaining the old digest, then assert rejection. The focused test passed. Artifact implementation was not broadly re-reviewed, per scope.

## Dependency notice check

The generated notice check passed. Comparing the baseline and working notice inventory found only five added package rows: flate2, miniz_oxide, crc32fast, adler2 and simd-adler32. There were no removed package rows, and the mapped existing license text bodies were unchanged. Existing dependency headers were regrouped by regeneration, which makes the textual diff larger than the package delta. data-encoding was already present in the inventory.

## Validation and limits

- Passed: focused P05 protocol test, one artifact linkage digest test, scripts/license-inventory --check, and scoped git diff --check.
- The task records report the compressed-bootstrap/session and loopback SSH focused tests passing; not independently rerun here because the active session-runtime lib.rs was excluded from inspection and broader integrated runs were out of scope.
- No linked Ghostty execution, hosted artifact, full scripts/check, native build or delivery gate was run. No overall merge or delivery verdict is implied.
