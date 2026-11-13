# Universal native file-panel load diagnosis

**Verdict:** Run `43418903541` reached the packaged file-panel smoke and failed its wait for all sixteen tabs to report loaded. The available records do not identify which layer prevented that condition. The timeout is not evidence that the Swift gate exceeded its limit or that the Rust file worker rejected an attach.

This was a read-only source and diagnostic review at `ab789718c1cea30b8cf610f9839bb1b1b1b665ad`. No source or tests were changed, no workflow/build or test was run, and no app was launched. Only this reference is authored.

## Hosted evidence

- The stage receipt `universal-native-diagnostics-extracted/native-smoke.json.stages.json` is pinned to source `ab789718c1cea30b8cf610f9839bb1b1b1b665ad`, with `currentStage=file_panel`, `state=failed`, and failure `timed out waiting for sixteen simultaneous file tabs loaded`. The completed-stage list includes browser, native render, SSH reconnect, local tmux, download checks, and other earlier work.
- In `universal-development-job-131865014573-full.log`, line 193 reports the standalone File Load Gate preflight passed: sixteen scripted requests, peak three in flight, FIFO waiting, the 250/500/1000 ms busy retry schedule, and its failure/cancellation cases. Line 2573 records the native smoke failure. The native smoke step failed before the later sidebar/status-menu and final artifact-upload steps.
- The Rust test log records `the_sixteen_file_open_needs_every_earlier_file_tab_closed_and_the_smoke_closes_them` and `the_swift_load_gate_keeps_sixteen_tabs_inside_what_the_rust_worker_admits` passing (lines 1312 and 1315). These are useful bounded owner checks, not proof that the packaged AppKit path completed.
- Workspace reorder passed at full-log line 2537. The scheduled-message smoke receipt reports all sixteen checks passed. The `EPERM` process-cancellation messages elsewhere in stderr are separate observations and do not explain this file-panel wait.
- The app trace has no per-tab attach, gate, or load-result records. It does contain the expected dirty-close refusal exercised by `FilePanelSmoke.swift:294-301`; the source intentionally queues a close of a dirty file, expects the core to refuse it, and checks the text remains. That message is not evidence of the later batch failure. Other browser and window-focus trace entries are not tied to the file load predicate.
- The retained `Zmux-native-smoke.png` shows the earlier terminal/browser fixture, not the file-panel state at timeout. There is no failure-state file-panel screenshot or second app launch in the extracted diagnostics.

## Production path and owners

The smoke creates sixteen unique small text fixtures, closes the three earlier file tabs, and invokes one real `zmux open` command (`apps/macos/Sources/FilePanelSmoke.swift:397-414`). The CLI sends one `TargetOperation::OpenItems` request (`apps/cli/src/open.rs:18-63,175-202`; dispatch in `apps/cli/src/main.rs:248-250`). The Rust control handler probes the items, then submits one `Command::OpenFiles` batch (`crates/desktop-runtime/src/control.rs:1188-1235`; owner methods in `crates/desktop-runtime/src/lib.rs:1329-1357,1405-1432`). The smoke's pre-wait guard observes `manyOpened.ok == true` and sixteen result items at `FilePanelSmoke.swift:406-408`.

After that batch updates the model, the native path is:

1. `ApplicationCoordinator.reconcileFilePanels` creates/reuses a `FilePanelView` for every file surface and calls `update` (`apps/macos/Sources/ApplicationCoordinator.swift:885-911`).
2. A new idle view starts an attach when it receives its file snapshot (`apps/macos/Sources/FilePanelView.swift:458-485,556-585`). `sendLoad` routes the attach through the shared `FileLoadGate` (`FilePanelView.swift:532-547`), which permits three whole-file requests at once and retries only known transient failures up to four attempts with 250/500/1000 ms back-off (`apps/macos/Sources/FileLoadGate.swift:56-83,177-257`).
3. The coordinator forwards the request to `NativeBridge.file`; that method performs the blocking C call on a detached worker (`ApplicationCoordinator.swift:1018-1024`; `apps/macos/Sources/NativeBridge.swift:553-573`). The bridge parses and executes the attach through `file_channel::execute` and `Desktop::file_attach` (`crates/native-bridge/src/lib.rs:996-1023`; `crates/native-bridge/src/file_channel.rs:146-205`; `crates/desktop-runtime/src/lib.rs:1433-1442`).
4. Rust `FileService::attach` validates the epoch/surface/host, enqueues a load on its bounded worker, and waits for the owner reply (`crates/desktop-runtime/src/files.rs:35-44,403-429,603-620,669-718`). A successful reply is applied by the Swift view; text becomes loaded after UTF-8 decoding and view update (`apps/macos/Sources/FilePanelView.swift:567-649`).

The fixture's `cli(["open"] + manyPaths, ...)` uses the CLI/control-socket route for surface creation. It does not use the coordinator's separate `.open` `FileCommand` path. The view attaches do use the native file bridge described above.

## What the packaged run proves

The sequential smoke reached the assertion after the one-command open and passed the guard that required a top-level successful result with sixteen items. It had also passed the earlier file-panel journey steps, including the mixed real-CLI open and close/reload flows, because the timeout is thrown at the later wait.

It did **not** record that sixteen matching surfaces had arrived, that sixteen views existed, that any particular view loaded or failed, that the gate drained, or that the peak stayed at three in this process. Those checks are after the wait at `FilePanelSmoke.swift:415-425` and were not reached. The wait predicate at lines 410-414 requires sixteen matching surfaces and `isLoaded == true` for each. A failed attach remains not-loaded, so the later `failureCode` assertion at line 419 cannot report that failure: the predicate simply waits until the existing 15-second deadline (`FilePanelSmoke.swift:510-515`) and emits the generic timeout.

Coverage boundaries:

- `scripts/check-file-load-gate` compiles the production Foundation-only gate with `tools/native-fixtures/file-load-gate.swift`. That fixture drives a scripted service and verifies the gate's real scheduler, retries, cancellation and sixteen-request behavior. It does not execute AppKit, the coordinator, `NativeBridge.file`, or the real Rust service.
- `the_swift_load_gate_keeps_sixteen_tabs_inside_what_the_rust_worker_admits` reads Swift policy values, opens sixteen surfaces, then directly drives the Rust FFI with its own bounded worker threads (`crates/native-bridge/tests/swift_file_contract.rs:1878-2025`). It validates the modeled Rust capacity, not the running Swift scheduler or snapshot-to-view reconciliation.
- `FilePanelSmoke` is the packaged owner-boundary integration for the real CLI, AppKit views, gate, bridge and Rust owner. This exact run reached it, but its timeout did not preserve the per-view state needed to identify a defective owner.

## Authoring gate for a fixture diagnostic change

1. **Observable contract:** one real batch of sixteen file paths produces sixteen file tabs that all load, with at most three whole-file requests in flight and no gate work left over. Keep the existing success checks and 16-path batch.
2. **Credible regression:** a surface is absent from the polled snapshot, has no corresponding native view, its attach fails or stalls, or gate work remains queued/in flight. Any of these can leave the current all-loaded predicate false while obscuring the first useful state.
3. **Why current coverage is insufficient:** the standalone gate test uses a scripted service; the Rust test models the configured concurrency directly; neither observes the packaged coordinator/view/bridge interleaving. The packaged smoke is the correct owner boundary. Improve the existing failure report instead of adding a duplicate contract test.
4. **Production seam:** none is needed. The fixture already has `coordinator.snapshot`, `coordinator.files`, `FilePanelView.isLoaded`, `failureCode`, and `presentationSummary` (`FilePanelView.swift:80-82,191-202`), plus gate current/peak counters (`FileLoadGate.swift:200-205`). Do not add a production-only hook or bypass the real path.

## Smallest robust diagnostic

Keep the existing 15-second deadline, single real CLI batch, all sixteen paths, gate limit, retry policy, and assertions. When the existing wait expires, append a bounded summary built from the already available snapshot/views/gate:

- For each fixture operand, report only its fixed index and one status: `surface_missing`, `view_missing`, `loaded`, `loading`, or `error:<sanitized stable code>`.
- Include the matching surface count and gate `inFlightCount`, `waitingCount`, `peakInFlightCount`, and `peakWaitingCount`.
- Do not log paths, file contents, surface/host IDs, UUIDs, or arbitrary failure messages. Cap/sanitize any included error code and keep the diagnostic to the fixed sixteen entries.

Compute this summary only after the original wait fails; do not change the predicate or its deadline. This should distinguish snapshot/reconciliation gaps, explicit attach failures, and unfinished gate work without inventing a production seam. The present artifact cannot tell which of those states occurred, so this is a diagnosis aid, not a claimed repair or root-cause finding.

## Proof limits and handoff

No local macOS re-execution, source/test edit, rebuild, or workflow rerun was performed. The hosted run is exact-source packaged evidence for the first native file-panel failure only; it is not app delivery or final artifact acceptance. The root cause remains unlocated pending the bounded per-tab/gate state at the unchanged timeout. Parent owns design acceptance and all subsequent repair and delivery work.
