# Packaged native schedule receipt diagnosis

**Date:** 2026-11-13 UTC
**Hosted source:** `428e47d42d8b6474cbb1770c003e3b22995fd144`
**Run/job:** `43376189745` / `131724168940`
**Scope:** Read-only diagnosis of the packaged scheduled-message receipt smoke. No source, test, script, build, workflow, or app was changed or run for this review.

The checkout `HEAD` is the hosted source SHA, and all traced production and fixture files match that commit.

## Finding

The failure is localized more narrowly than “the receipt was not found”: after the smoke deliberately lost the create reply and reached its unknown-outcome UI, the receipt-status action left that UI unresolved. The screenshot's clipped status begins `Native scheduleRequest(identityNamespace:surface:expectedSession:request:) failed...`. That text matches the `NativeBridge.failure` path for a nonzero return from `zmux_desktop_schedule_request`, before Swift decodes its response buffer.

This points to the native bridge ABI boundary, but does **not** identify its return status or prove whether the read-only status request reached the Desktop route or remote helper. A nonzero ABI result can come from pre-execution bridge admission/deserialization, or from result publication after the Desktop request has run. The UI clipped the numeric status, the report did not save it, and the fixture's request log was deleted during cleanup. No production or fixture race is established. No functional repair is supported yet; obtain the missing failure evidence first.

## Exact observed interleaving

1. The hosted step ran the built app's `--schedule-messages-smoke` path with the scheduler fixture's private helper and managed SSH fixture. Initial status/target/list/history refresh succeeded; the smoke selected the remote target and reported `Remote scheduler refreshed`.
2. `ScheduleMessagesSmoke.swift:212-240` creates a unique message, writes the one-shot drop marker immediately before clicking Save, then waits for the pending mutation to enter unknown-outcome UI. The recorded checks include `lost_create_reply_enters_unknown_outcome_ui`, `unknown_outcome_ui_exercised=true`, and a removed marker. This establishes that the intended lost-reply scenario was exercised; it does not by itself establish the create receipt or later status request result.
3. The smoke clicks **Check Request Status** once (`ScheduleMessagesSmoke.swift:240`) and waits for up to 30 seconds for a single populated, selected row, the expected deadline/state/editor values, and the unresolved-status button to disappear (`:241-253`, `:906-914`). That composite predicate never became true. The outer failure therefore says only that the complete UI predicate did not pass.
4. `ScheduleMessagesWindow.swift:1020-1095` sends `Status { receipt_request_id }`, verifies the matching receipt, reads the job, and refreshes the queue. Its catch calls `present(error)` and ends work. It does not replay the create. The screenshot shows the empty, not-yet-refreshed table, the unresolved status button, and a clipped native `scheduleRequest` error. The table's `No pending messages` text is stale UI from before this failed status action; it is not evidence that the remote scheduler has no job.
5. The report at `/tmp/zmux-osc7501-final-428e47d/native-diagnostics-extracted/native-schedule-messages-smoke.json` stores the generic timeout, the five checks completed before this step, and the screenshot filename. The smoke stderr and full job log likewise contain only the outer timeout at 06:38:00 UTC. The screenshot is `/tmp/zmux-osc7501-final-428e47d/native-diagnostics-extracted/zmux-scheduled-messages.png`.

## Source evidence and branch boundary

- `apps/macos/Sources/NativeBridge.swift:255-288,847` calls `zmux_desktop_schedule_request`; a nonzero `result.status` throws `Self.failure(result.status)` at `:278`. Its message format is `Native <operation> failed (<status>)`, matching the visible screenshot prefix. On status zero, the method consumes and validates the JSON wrapper.
- `crates/native-bridge/src/lib.rs:1064-1082` reserves an output buffer, decodes `NativeRequest`, calls `desktop.native_schedule_request`, serializes `Execution`, and publishes the result. Relevant nonzero ABI statuses include invalid handle (`-1`), invalid argument (`-2`), busy registry (`-4`), buffer capacity (`-5`), and internal/publish failure (`-6`). The number is absent from retained evidence, so these remain candidate branches, not findings. Because publication follows the Desktop call, `-6` does not prove that route work was skipped.
- `crates/desktop-runtime/src/lib.rs:1117-1241` converts route capture and scheduler execution outcomes into `schedule::Execution`; route capture has its own two-second deadline. `crates/desktop-runtime/src/schedule.rs:201-280` converts remote command, transport, and protocol failures into that execution envelope. Those logical failures should reach Swift as a status-zero C-ABI result containing a JSON wrapper. Thus the screenshot makes a direct scheduler rejection, missing receipt, or SSH helper error less likely than an ABI-level failure, while the missing ABI status prevents choosing between the admissibility and post-execution publication paths.
- The UI's `perform` path in `ScheduleMessagesWindow.swift:543-583` maps a decoded error wrapper into `ScheduleBridgeFailure`. `checkUnresolvedStatus` catches either bridge or wrapper errors at `:1090-1093`. The screenshot's native function-name prefix is evidence of the earlier `NativeBridge` throw, not a returned scheduler error message.
- `crates/remote-runtime/src/scheduler/mod.rs:66-74,113-125` defines receipt lookup as a read-only Status action with an optional original request ID. The smoke uses a fresh status request with `receipt_request_id` (`ScheduleMessagesWindow.swift:1045`); it does not resubmit the original create. Preserve this no-replay/unknown-outcome behavior.

## Existing coverage to retain

- Hosted core job output records `schedule::ssh_tests::scheduler_rpc_uses_the_live_master_and_does_not_retry_a_lost_create_reply ... ok` at 06:15:08 UTC. This is an unusually relevant control, not a duplicate native test: `crates/desktop-runtime/src/schedule_ssh_tests.rs:18-42` invokes the same `scripts/native-ssh-fixture.py` fixture and one-shot drop; `:330-359` observes an uncertain create; `:361-383` obtains its `Applied` receipt over the same live managed master; `:385-399` asserts the create reached the helper exactly once. It proves the Rust scheduler transport plus shared drop fixture can reconcile successfully. It does not pass through `Desktop::native_schedule_request`, the C ABI, or AppKit, so it cannot establish the failing packaged boundary. This evidence lowers the likelihood of a generic fixture or remote scheduler defect.
- `crates/remote-runtime/src/scheduler/service/tests.rs:202-238,285-297` separately retains the store/service contract for original mutation receipts, idempotency, and read-only receipt lookup. Keep it as the scheduler owner contract; the packaged smoke protects the independent AppKit-to-native-bridge-to-managed-SSH path.
- Keep the packaged smoke's create-once/receipt-status UI assertion. It exercises a genuine release/package/platform contract and detects failures the Rust service and managed-SSH tests cannot reach. The issue is that its failure report currently collapses a surfaced status-action error into the generic 30-second predicate timeout, not that this acceptance contract is low-value.

## Fixture semantics and lost evidence

- `scripts/check-macos-app:386-389` enables scheduler RPC request logging and the one-shot post-execution response drop. `:495-503` launches the packaged smoke with a 180-second outer timeout. The failing inner wait is 30 seconds; no evidence supports raising either timeout.
- `scripts/native-ssh-fixture.py:101-125` checks and removes the marker once, captures the incoming RPC, executes the remote helper into a temporary response file, then kills the forced-command shell before that response reaches the client. Subsequent scheduler RPC commands take the ordinary `tee | helper` path. This fixture deliberately makes the create response unknown; it does not intentionally drop the later status response.
- The request log is stored under the private fixture root (`:94-100`) and contains raw scheduler JSON, including seeded message text and request identifiers. On any smoke failure the context manager stops the fixture and removes that root (`:203-209`) before the enclosing check reports/uploads diagnostics. The downloaded diagnostics contain no scheduler request log. Therefore we cannot tell from this run whether the later status request reached the helper. Do not retain the raw log in the hosted artifact.
- The retained `native-sshd.log` shows generic forced-command session starts/closes, but records neither commands nor scheduler action names. It cannot substitute for the removed request log or correlate a particular session with the receipt-status query.
- The reported lost-create state plus the fixture source argue against a marker that intentionally drops the status response. They do not rule out a route/connection or bridge problem triggered by the lost create reply, because there is no retained observation of the second RPC.

## Narrow next step

Keep the current single-create/unknown-outcome/status-only interleaving, receipt identity checks, no-replay guarantee, existing assertions, and 30-second smoke deadline. Do not add automatic retries, relax the populated-row assertion, or increase a global timeout.

Before making a functional repair, add bounded, sanitized failure evidence at the owning boundaries:

1. Preserve the C-ABI status as a stable symbolic class plus integer in a typed native bridge failure (for example, busy, capacity, invalid input/handle, or internal). Record the failing UI stage (`receipt_status_bridge`, `receipt_parse`, `show_job`, or `queue_refresh`) in the existing smoke JSON. Do not log request bodies, user text, route IDs, paths, SSH output, or keys.
2. Before fixture cleanup, convert the private scheduler request log into a small allowlisted summary: action counts/order and whether the receipt-status request arrived after the dropped create. Do not upload raw request lines or response contents.
3. Rerun the packaged macOS gate once on the instrumented source. Keep exactly one create and one receipt-status query. If the ABI class is pre-execution busy/capacity/input, investigate that bridge owner; if status zero returns a route/transport code, investigate route capture/SSH; if the status RPC is logged but the UI remains unresolved, capture the bounded receipt/show/refresh stage. Only then add a production-owner regression for the proven branch.

This Linux checkout cannot rerun the packaged macOS gate. The hosted failure is not macOS acceptance and does not establish a final artifact, Programs/sidebar acceptance, or live-user behavior.
