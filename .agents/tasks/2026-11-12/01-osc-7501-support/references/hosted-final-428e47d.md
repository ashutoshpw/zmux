# Hosted final verification for `428e47d`

**Date:** 2026-11-13 UTC
**Source:** `428e47d42d8b6474cbb1770c003e3b22995fd144` on `main`
**Verdict:** No final universal macOS development artifact was produced. The hosted run failed during packaged-app smoke, before native sidebar Programs acceptance.

## Workflow result

Run [43376189745](https://github.com/ashutoshpw/zmux/actions/runs/43376189745) used the exact requested source SHA. Both Linux scheduler jobs, Swift type-check, `Validate Rust core and foreign ABI`, `Build universal native development app`, and `Exercise terminal selection and browser links in the universal app` passed.

The first primary failure was job `131724168940`, step **Exercise packaged app and native rendering**, running `scripts/check-macos-app --skip-accessibility-save-tests`. At 06:38:00 UTC the packaged scheduled-message smoke failed with:

> `BridgeFailure(message: "Timed out waiting for applied create receipt selecting one populated job")`

The run’s `native-schedule-messages-smoke.json` records `state=failed`, the same source SHA and error, and `unknown_outcome_ui_exercised=true`; it names `zmux-scheduled-messages.png` as its failure screenshot. This records the observed failure and does not establish a root cause.

The subsequent **Exercise native sidebar and system status menu** and **Upload downloadable development build** steps were skipped. `native-sidebar-programs.json`, `native-sidebar.json`, `native-smoke.json`, and `zmux-sidebar-programs.png` are absent from the diagnostic artifact. Programs acceptance therefore did not run or pass, and there is no Programs image for visual inspection. No `zmux-macos-dev-universal-428e47d42d8b6474cbb1770c003e3b22995fd144` final artifact was uploaded.

The terminal-selection receipt does record `state=complete` and the exact source SHA. Its log includes a Python HTTP server `ConnectionResetError: [Errno 54] Connection reset by peer` immediately before the harness emitted the complete receipt. This is recorded as a log outcome; the selection step itself passed.

## Artifacts and retained diagnostics

- The run retained candidate artifact [14016316411](https://github.com/ashutoshpw/zmux/actions/runs/43376189745/artifacts/14016316411), `zmux-macos-candidate-universal-428e47d42d8b6474cbb1770c003e3b22995fd144`: API size **26,763,602 bytes**, API SHA-256 `16f87d54fb4addef3e9d48b4eb54232a2c0dce25711ec58befeb4fb11c235dc5`. It is a candidate only; it was not downloaded or accepted as a final artifact.
- Downloaded diagnostic artifact [14015788114](https://github.com/ashutoshpw/zmux/actions/runs/43376189745/artifacts/14015788114), `Zmux-native-diagnostics-428e47d42d8b6474cbb1770c003e3b22995fd144`: API size and downloaded size **30,189,815 bytes**; API and downloaded SHA-256 `b854fa41f043927ffbb2657c9058f1d1f264b67d499dc7ff185cdf0f285bd32e`. ZIP CRC passed. Safe extraction passed with **439 entries** and **37,503,276 expanded bytes**.
- Extracted diagnostics: `/tmp/zmux-osc7501-final-428e47d/native-diagnostics-extracted/`. Relevant files include `native-schedule-messages-smoke.json`, `native-schedule-messages-smoke.stderr.log`, `zmux-scheduled-messages.png`, and `native-terminal-selection-universal/receipt.json`.
- Full failed-job log: `/tmp/zmux-osc7501-final-428e47d/development-job-131724168940.full.log`, **342,072 bytes**, SHA-256 `efcc4fdb243474a0488cb48932dcdb9fcb764fe724063b567867c0a9c29c34a6`.

This run establishes hosted outcomes at the exact source SHA only. It establishes no final app delivery, Programs acceptance, Intel runtime, physical-Mac, production, or live-user acceptance. No workflow was dispatched, retried, or cancelled; no checkout, index, source, task status, or plan was changed by this verification.
