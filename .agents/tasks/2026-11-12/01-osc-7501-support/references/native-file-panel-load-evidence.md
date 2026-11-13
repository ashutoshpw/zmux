# Native file-panel timeout diagnostics

## Change

The existing sixteen-tab loaded wait in `apps/macos/Sources/FilePanelSmoke.swift` now augments only its `BridgeFailure` with an in-place snapshot taken before the smoke's existing cleanup. It retains the original failure message and error type. `CancellationError` is rethrown; an active task cancellation takes precedence over adding diagnostics, and any other error is rethrown unchanged.

The diagnostic reports, in operand order, one of the fixed statuses `surface_missing`, `view_missing`, `loaded`, `loading`, or `error:<code>` for each of the sixteen generated paths. Error codes are limited to 32 UTF-8 bytes and non-ASCII or non-identifier bytes are replaced with `_`; empty codes become `unknown`. It also reports the matching surface count and existing `FileLoadGate` in-flight, waiting, peak-in-flight, and peak-waiting counts. It logs no paths, file contents, IDs, UUIDs, or arbitrary per-view messages.

The wait predicate, 15-second deadline, batch open, path count, concurrency and retry policy, and all post-wait success assertions are unchanged. The diagnostic uses existing coordinator snapshot/view/gate state and adds no production seam.

## Test-authoring gate

1. **Observable contract:** one real CLI batch of sixteen paths creates sixteen file tabs that all load, while no more than three whole-file requests are in flight and the gate drains. The existing smoke continues to assert this contract.
2. **Credible regression:** one or more tabs fail to appear in the snapshot, lack a coordinator view, fail their attach, remain loading, or leave gate requests queued/in flight. The existing all-loaded wait collapses each of these outcomes into the same timeout.
3. **Why existing coverage does not catch it:** the standalone Swift gate fixture uses a scripted service, while the Rust bridge contract test applies concurrency through its own bounded callers. Neither executes the packaged coordinator-to-view reconciliation and actual gate/bridge/Rust sequence. The existing packaged `FilePanelSmoke` is the owner-boundary test; this change improves its failure evidence without adding a duplicate test.
4. **Production seam:** none. The fixture reads `coordinator.snapshot`, `coordinator.files`, `FilePanelView.isLoaded`/`failureCode`, and the existing gate counters. No production behavior or test-only production API is added.

## Evidence and limits

Run `43418903541` at source `ab789718c1cea30b8cf610f9839bb1b1b1b665ad` passed the separate gate preflight and the Rust tests `the_sixteen_file_open_needs_every_earlier_file_tab_closed_and_the_smoke_closes_them` and `the_swift_load_gate_keeps_sixteen_tabs_inside_what_the_rust_worker_admits`. The packaged smoke's real CLI batch returned a top-level successful result with sixteen items, then its wait for all tabs to load timed out. Its captured trace did not retain per-tab load states or gate counters. No failure-state screenshot was available.

The original cause remains unknown. This change only makes the next timeout distinguish snapshot/view gaps, explicit load errors, and outstanding gate work. It does not establish which state occurred in the earlier run and does not claim a runtime fix.

## Validation

- `cargo test --locked --offline -p zmux-native-bridge --test swift_file_contract`: **33 passed, 0 failed**. This includes the existing sixteen-file capacity and Rust-owner contract checks; no tests were added.
- `git diff --check -- apps/macos/Sources/FilePanelSmoke.swift`: passed.
- `python3 scripts/check-macos-sources`: unavailable here; the script exited at its platform guard with `Native source checks require macOS and its SDK`.
- `swiftc -frontend -parse apps/macos/Sources/FilePanelSmoke.swift`: unavailable because `swiftc` is not installed. No Swift typecheck or AppKit execution is claimed.
- No full `scripts/check`, app build, or workflow was run.
