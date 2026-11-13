# Native schedule receipt evidence recheck

**Date:** 2026-11-13 UTC
**Compared source:** `428e47d42d8b6474cbb1770c003e3b22995fd144`
**Scope:** Read-only review of the uncommitted `ScheduleMessagesSmoke.swift` and `scripts/check-macos-app` diagnostic slice against `native-schedule-receipt-diagnosis.md` and `native-schedule-receipt-evidence.md`.

## Verdict

The diagnostic slice is scoped correctly and adds no scheduler repair or delivery claim. It retains the existing create-once and status-only interleaving, the populated-row assertion, and the existing 30-second smoke and 180-second outer bounds. I found one low-severity hard-bound gap in the report/configuration reads below. No functional, raw-data disclosure, replay, or receipt-versus-show certainty finding was identified.

## Finding

**P3 — Report and fixture-configuration reads are only preflight-bounded.** In `scripts/check-macos-app:390-392` and `:414-418`, `stat()` checks the report's 1 MiB and fixture configuration's 64 KiB limits, then `read_text()` reads each whole file without an actual byte limit. If either file grows or is replaced between the check and read, the configured bound does not limit the bytes consumed. The request log handles this race correctly with `read(1 MiB + 1)` and rejects an over-bound read at `:423-429`. The initial report parse also does not catch `RecursionError`, so a deeply nested report can escape the helper instead of returning `False` for the caller's fixed unavailable warning.

This is a defensive bound gap for generated local files; the fixture configuration is written once and the current smoke report is small. It does not affect the privacy properties of the allowlisted summary. No source change was made as part of this review.

## Source checks

- The smoke still clicks **Check Request Status** once and waits on the original populated, selected row predicate (`ScheduleMessagesSmoke.swift:240-257`). The edit only attaches evidence if that wait throws. The create marker and generated test messages are unchanged.
- The stage classifier uses the current visible status strings present in `ScheduleMessagesWindow.swift`: receipt lookup at `:1023-1025`, receipt verification at `:1085`, queue refresh at `:383`, and reconciliation at `:476`. The native error grammar is produced by `NativeBridge.failure` (`NativeBridge.swift:847`) when `scheduleRequest` sees a nonzero ABI result (`:278`). The native parser accepts only a complete canonical nonzero `Int32` line with that exact prefix and suffix; it stores only the integer and coarse class.
- The ABI constants at `crates/native-bridge/src/lib.rs:41-46` are `-1` invalid handle, `-2` invalid argument, `-3` buffer too small, `-4` busy, `-5` capacity, and `-6` internal. The diagnostic mapping groups `-3` and `-5` as `capacity`, maps `-4` to `busy`, and maps other values, including `-6`, to `internal`.
- A native error is reported as `receipt_status_or_show`; the code comments correctly state that the same production bridge call covers receipt lookup and the following show request. Queue refresh and reconciled stages require their respective visible status strings. There is no claim that a nonzero ABI result proves the request did not execute.
- The RPC summary emits fixed action categories, counts, at most 128 action labels, records up to the 1 MiB log bound, and treats lines over 16 KiB as malformed. It does not copy request values, raw lines, user text, IDs, routes, paths, SSH output, or keys into the summary. The one-shot marker and unknown-outcome UI are only corroborating fixture evidence; the action log cannot prove durable receipt contents or response delivery to the app.
- No request replay/status retry, wait extension, assertion weakening, production seam, workflow change, or raw request-log retention was added. Both failure and success paths attempt to attach the summary before fixture cleanup and print a fixed warning if the helper returns `False`.

## Verification boundary

No tests, native builds/apps, workflows, commits, pushes, or broad scheduler checks were run by this reviewer. The parent is running `scripts/check` on stable source. This Linux source review cannot establish Swift compilation, packaged macOS behavior, helper response delivery, fresh hosted acceptance, or artifact state. The parent reported the existing temporary-input action-count/privacy check and Python syntax check passed; those results were not independently rerun here.
