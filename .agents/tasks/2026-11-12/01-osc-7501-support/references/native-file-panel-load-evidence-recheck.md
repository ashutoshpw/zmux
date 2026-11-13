# Independent review: native file-panel timeout diagnostics

## Verdict

**No actionable defect found in the scoped change.** The timeout path adds bounded state evidence without changing the open batch, wait predicate, deadline, or successful-path assertions. This is a source-level review of `main` at `ab789718c1cea30b8cf610f9839bb1b1b1b665ad` plus the current `FilePanelSmoke.swift` worktree diff. No unrelated worktree paths were changed.

## Severity-ranked findings

None.

### Informational diagnostic limit

`FilePanelSmoke.swift:480-492` emits one row per expected input path and selects the first matching surface for that path. If an anomalous snapshot contains duplicate surfaces for one expected path, the aggregate `matching_surfaces` count exposes the cardinality mismatch, but the extra surface's individual state is not shown. This does not affect the intended sixteen unique paths or the existing wait predicate; it is a limit on the diagnostic detail in that unusual state.

## Review evidence

- **Error and cancellation grammar:** `FilePanelSmoke.swift:555-560` shows that this wait's own deadline failure is `BridgeFailure` from `failure(...)`, while its only throwing suspension is `Task.sleep`, which can propagate `CancellationError`. The new catch at `:416-423` rethrows an existing `CancellationError`, checks `Task.isCancelled` before adding diagnostics, augments only `BridgeFailure`, and rethrows any other error unchanged. `BridgeFailure` stores only `message` (`NativeBridge.swift:26-29`), so reconstructing it retains its existing error payload and type while adding the suffix.
- **Error-code source and grammar:** the view exposes `failureCode` as `String` and assigns it from `FileHostFailure.code` (`FilePanelView.swift:80-82, 711-718`). `FileHostFailure` documents the code as stable Rust vocabulary or `transport` (`FileHostClient.swift:43-60`); decoding copies the wire code into that string, with `invalid_reply` as the fallback (`FileHostClient.swift:168-191`). Since the Swift field is not locally constrained to an identifier grammar, sanitizing the UTF-8 bytes is appropriate. `boundedDiagnosticCode` retains at most 32 bytes, emits only ASCII letters, digits, and underscore, and substitutes `unknown` for empty input (`FilePanelSmoke.swift:498-506`).
- **Privacy and output bounds:** the caller creates and successfully opens sixteen generated paths before entering this wait (`FilePanelSmoke.swift:405-408`). The diagnostic uses those paths only for matching and emits fixed indices, fixed status labels, sanitized error codes, matching count, and integer gate counters (`:474-495`). It does not interpolate paths, file contents, identifiers, or arbitrary per-view error messages. The status list has one entry per input path; its only variable per-entry field is capped at 32 bytes.
- **Actor isolation and Swift 6 conventions:** `FilePanelSmoke` is `@MainActor` (`FilePanelSmoke.swift:27-28`), as are `ApplicationCoordinator` (`ApplicationCoordinator.swift:57-59`), `FilePanelView` (`FilePanelView.swift:50-52`), and `FileLoadGate` (`FileLoadGate.swift:177-205`). Both new helpers explicitly use `@MainActor`; snapshot, view, and gate reads remain synchronous on that actor. Source inspection found no apparent Swift 6 isolation or `Sendable` issue. A macOS Swift type-check was not available to this review.
- **Successful and failure semantics:** the original predicate at `FilePanelSmoke.swift:411-415` remains unchanged, including the sixteen-surface/all-loaded condition. The same single CLI batch and sixteen-item success guard remain at `:405-408`; the existing no-error, concurrency-limit, and drained-gate assertions remain at `:424-435`. Diagnostic construction happens only after that wait throws and introduces no await or production-state mutation.
- **Existing ownership and test evidence:** the coordinator reconciles file surfaces into retained views at `ApplicationCoordinator.swift:885-911` and owns the shared gate at `:1018-1025`; the gate exposes the current and peak counters used by the diagnostic at `FileLoadGate.swift:200-205`. No new test or production seam was added. The existing packaged `FilePanelSmoke` remains the owner-boundary integration described in `native-file-panel-load-diagnosis.md:35-56`.

## Proof limits and next step

The related evidence records the earlier packaged run timing out without per-tab state and leaves its root cause unknown (`native-file-panel-load-diagnosis.md:7-14, 29-39`). It also records the author's Rust `swift_file_contract` result of 33 passed (`native-file-panel-load-evidence.md:24-30`); I did not rerun it. No build, Cargo command, test, or workflow was run for this review. The references report that Linux lacks `swiftc` and the macOS SDK, so this review cannot confirm Swift 6 compilation or AppKit execution. The parent `scripts/check` run was already in progress and is not represented here as a completed result. A later packaged macOS run is still required to observe whether the new failure summary identifies the state behind another timeout; this change itself does not establish a runtime fix or native acceptance.
