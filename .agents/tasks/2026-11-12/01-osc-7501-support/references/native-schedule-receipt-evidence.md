# Native schedule receipt diagnostic slice

**Date:** 2026-11-13 UTC | **Hosted baseline:** `428e47d42d8b6474cbb1770c003e3b22995fd144`
**Purpose:** Retain bounded failure evidence for the existing packaged create-once / receipt-status acceptance path. This is diagnostic instrumentation only; it does not repair scheduler behavior.

## Evidence added

The existing smoke report, `native-schedule-messages-smoke.json`, gains `receipt_status_diagnostics` only when the existing populated-row wait fails:

```json
{
  "receipt_status_diagnostics": {
    "ui_stage": "receipt_status_or_show",
    "ui_stage_source": "native_schedule_request_error",
    "native_abi": { "status": -4, "class": "busy" }
  }
}
```

`ui_stage` is one of `receipt_status_or_show`, `queue_refresh`, `receipt_reconciled`, or `null`. The smoke reads only fixed, visible status text. It reports `receipt_status_or_show` when the visible status lookup text or exact native bridge error is observed; the same bridge call is used for the receipt lookup and the following `show`, so the UI cannot distinguish those requests. It reports `queue_refresh` only after the UI visibly says receipt verification completed or queue refresh started, and `receipt_reconciled` only for the visible reconciliation status. Otherwise the stage is `null`. `ui_stage_source` is `visible_status`, `native_schedule_request_error`, or `none`.

`native_abi` is `null` unless visible status text exactly matches the production grammar `Native scheduleRequest(identityNamespace:surface:expectedSession:request:) failed (<canonical nonzero Int32>)`. The numeric code is retained with one stable class: `-1` maps to `invalid_handle`, `-2` to `invalid_input`, `-4` to `busy`, `-3` and `-5` to `capacity`, and other values to `internal`. The complete visible message is never copied into the report. No `NativeBridge` API or UI behavior changed.

Before the SSH fixture context cleans its private root, `scripts/check-macos-app` transforms the raw scheduler request log and merges this allowlisted summary into the same report:

```json
{
  "scheduler_rpc_summary": {
    "available": true,
    "reason": null,
    "action_counts": {
      "status": 1,
      "receipt_status": 1,
      "targets": 1,
      "list": 1,
      "history": 1,
      "setup": 0,
      "setup_check": 0,
      "create": 1,
      "show": 0,
      "edit": 0,
      "cancel": 0,
      "other": 0
    },
    "action_order": ["status", "targets", "list", "history", "create", "receipt_status"],
    "action_order_truncated": false,
    "records_seen": 6,
    "malformed_records": 0,
    "dropped_create_observed": true,
    "create_count_before_first_receipt_status": 1,
    "receipt_status_arrived_after_dropped_create": true
  }
}
```

Only fixed action labels are emitted: `status`, `receipt_status`, `targets`, `list`, `history`, `setup`, `setup_check`, `create`, `show`, `edit`, `cancel`, and `other`. The sequence is capped at 128 labels; the log is bounded to 1 MiB and each record to 16 KiB. Receipt-status classification checks only whether `receipt_request_id` is a string and discards its value. `dropped_create_observed` also requires the existing smoke's `unknown_outcome_ui_exercised` evidence, a consumed one-shot marker, and a logged create. The after-drop field is `null` when that interleaving was not observed. Missing or over-bound fixture data produces a fixed unavailable reason and does not fail the acceptance run.

The summary stores no request bodies, messages, text, IDs, routes, paths, SSH output, or keys. It is written to the report already uploaded by the macOS workflow; no workflow change is needed.

## Acceptance and proof limits

- The existing packaged smoke remains the sole owner of the create-once, unknown-outcome, receipt-status, populated-row assertion and its existing checks. No new test, mock, production seam, retry, wait extension, or assertion change was added.
- The four test-authoring gate answers are: it protects the existing observable packaged acceptance and adds failure evidence; a bridge, response-publication, or scheduler-route regression can otherwise collapse to the same timeout; existing Rust/service coverage does not reach this AppKit/native-bridge/managed-SSH boundary and the existing smoke is retained; and no production seam is added.
- The RPC summary proves only which allowlisted request actions reached the private SSH fixture and their order. It cannot prove remote durable receipt contents, the response reached the app, or which post-ABI action completed.
- A nonzero ABI status still does not alone prove whether the request executed before result publication failed. No functional repair is supported by this slice.
- The source was changed after hosted run `43376189745`; the next hosted packaged macOS experiment is required. This Linux host has no Swift compiler, so native compilation, packaged UI behavior, artifact upload, and the fresh experiment remain pending.

## Parent review follow-up

Independent review found no replay, privacy or classification issue. Parent
repaired its P3 by capping actual report and configuration reads and catching
report parse/serialization recursion errors. The request log read is also
capped; failure to attach diagnostics emits a fixed warning. Temporary owned
fixtures passed action-order/private-value exclusion, deep report, oversized
report and oversized configuration checks. No new test suite or app behavior
was introduced. The final aggregate retry follows these changes.
