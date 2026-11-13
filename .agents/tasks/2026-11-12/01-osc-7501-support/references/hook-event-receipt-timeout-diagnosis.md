# Managed SSH hook-event receipt timeout diagnosis

**Date:** 2026-11-13 UTC
**Source:** `6230bd4fb6842d15901f1c02daab7fa91dfb9dea` (`main`; current `HEAD`)
**Hosted run/job:** universal development run `43398619986`, job `131797064381`
**Scope:** Read-only diagnosis of the failed `zmux-remote-runtime --test connection` case. This reference is the only created file. No source, test, task status/plan, index, build, workflow, app, commit, or push action was performed.

## Finding

The failure is an `EAGAIN`/`WouldBlock` from the test-owned Unix datagram socket at `crates/remote-runtime/tests/connection.rs:547-549`. The socket has a 250 ms read timeout installed at `:463-465` for the stale-receipt negative control, and that same timeout remains active for the later valid-receipt read. On macOS, expiry of this socket receive timeout is reported as `WouldBlock` (errno 35). The log therefore proves only that no datagram reached this test socket during that 250 ms receive window.

That window is shorter than the receipt budget represented by the fixture and production hook. The synthetic event writer supplies `receipt_deadline = time.monotonic() + 1.5` at `connection.rs:479`. The production Claude hook uses `RECEIPT_WAIT_SECONDS = 1.8` and puts that absolute deadline on alert-bearing events (`crates/session-runtime/src/shell/claude-lifecycle-hook.py:16,328-350`). The hook waits only until the remaining time to that deadline (`:198-231,382-385`). The stream admits a required receipt only while its supplied deadline is in the future and no more than three seconds away, then retains the pending entry until that exact deadline (`claude-lifecycle-stream.py:687-712,362-365`). There is no production 250 ms event-receipt deadline.

The test’s 250 ms positive receive is thus an expectation mismatch. The synthetic 1.5 s event deadline is also 300 ms shorter than the production hook’s configured budget. Either can cause a false test failure under hosted scheduling; the retained log has no per-stage timestamps to determine whether the valid receipt was sent after the test socket timed out, after the event expired, or not sent at all. The evidence supports correcting the test’s deadline accounting. It does not establish a production receipt-delivery defect.

## Receipt lineage and boundary

1. The test creates one `OpenRequestNonce`, registers it as `ManagedSsh`, passes it into the PTY/bootstrap, and asserts the active Session registration retains that nonce (`connection.rs:280-310`). It launches the generated lifecycle stream through the managed SSH connection (`:331-348`), checks the Hello version and nonce, sends Activate with a fresh generation, then checks the Active version, nonce, and exact generation (`:349-379`).
2. Earlier synthetic Hook events are read from the live stream and checked for strictly increasing sequence numbers (`:409-455`). For the receipt case, the test binds a private `r-<event-id>` datagram endpoint with mode `0600`, creates one Notification event, and writes a record carrying the same lifecycle nonce, event UUID, `receipt_required=true`, `receipt_requested=true`, and the synthetic 1.5 s deadline (`:457-495`).
3. The test receives the Hook frame and verifies its kind, sequence progression, exact event ID, requested flag, and active generation (`:496-509`). This proves the stream admitted this pending event and emitted its matching lineage. The FIFO writer supplies a synthetic event record; the real remote lifecycle stream implementation is exercised, while the per-hook socket is owned by the test.
4. A first ClientFrame uses the correct nonce and event ID but a deliberately different generation. Its lack of datagram is checked under the 250 ms negative-control timeout (`:511-532`). The next ClientFrame uses the exact active generation, nonce, event ID, protocol version, and `HandledNoAlert` disposition (`:534-546`). Its following `recv` inherits the same 250 ms timeout (`:547-549`), then the test asserts the returned datagram’s kind, event ID, generation, disposition, and nonce (`:550-561`).
5. On the remote owner, `process_input_bytes` parses each newline-framed client message and calls `valid_receipt` (`claude-lifecycle-stream.py:341-359`). That owner rejects wrong field sets, version, nonce, generation, disposition, or unknown event ID; it also rejects a receipt at or after the pending deadline (`:243-271`). A matching live receipt is sent to the private per-hook socket (`:272-283`), whose owner and mode are checked before `sendto` (`:174-193`). The stream polls stdin at up to 100 ms intervals (`:522-535`).

There is no further stream acknowledgement after the datagram send. The test’s `recv` is the delivery observation; it does not run the production hook process as the socket receiver or prove that hook’s fallback handling completed. The earlier `remote_notice` check executes a real notice hook but tests its attention fallback, not this receipt path (`connection.rs:380-405`).

## Why this does not currently indicate a nonce or framing mismatch

- The test uses the registration nonce for Hello, Activate, FIFO event, and both ClientFrames. It echoes the active generation for the valid frame and intentionally changes only that field in the stale control.
- The received Hook frame has the exact event UUID, `receipt_requested=true`, and the active generation before the valid frame is sent.
- `ClientFrame::Receipt` serializes the fields accepted by `valid_receipt`: `kind`, `version`, `nonce`, `generation`, `event_id`, and `disposition`. The valid disposition is in the owner’s accepted set.
- The positive read fails before parsing or checking any datagram. Its error cannot distinguish late send, expired pending state, socket-send failure, or a production parser mismatch. The remote helper currently suppresses `OSError` from the datagram send (`claude-lifecycle-stream.py:281-282`), so the test log contains no send-failure class.

Nothing in this trace enters the CLI’s pre-spawn `ProviderRouteStarted` admission/fallback path. Keep that separate route-admission investigation with its current owner; this test failure does not establish a shared cause.

## Smallest evidence-supported next step

Keep the existing loopback managed-SSH integration test as the positive owner-boundary regression. Preserve its stale-generation no-delivery assertion and every exact nonce, generation, event-ID, sequence, disposition, and datagram-field assertion. Do not add a second test for the same boundary.

Correct only the fixture’s timing contract: scope the 250 ms wait to the stale-generation negative control, and make the positive receive bounded by the remaining absolute receipt deadline that the synthetic event actually supplied. Keep that deadline aligned with the production hook’s 1.8 s budget rather than inventing a larger transport budget. If the exact receipt is not delivered before that owner deadline, the regression should still fail. Do not remove either assertion, sleep to make it pass, or increase a production timeout.

If the test still fails with time remaining at the valid send, the current hosted evidence is insufficient to choose between input validation, expired/missing pending state, and datagram send failure. Add a bounded, sanitized diagnostic at the stream owner or test fixture that reports only the phase and reason class (for example: frame parsed, pending absent/expired, identity rejected, endpoint rejected, send failed, or sent) plus elapsed/remaining milliseconds. Do not include nonce, generation, event ID, socket path, event payload, or SSH output. Then rerun this same owner-boundary regression; no source-level transport repair is supported until the class is observed.

## Test-authoring gate and proof limits

- **Contract:** a valid lifecycle receipt for the active nonce/generation and exact pending event is routed to that event’s private socket before its deadline; a stale generation is not routed.
- **Credible regression:** receipt parsing/lineage/pending validation or private endpoint forwarding drops a valid receipt or forwards a stale one.
- **Existing owner coverage:** this real loopback managed-SSH stream test is the strongest positive transport boundary found. `required_remote_receipt_failures_emit_a_generation_gap` covers expired/invalid required events locally (`connection.rs:811-875`), but not positive exact receipt forwarding. Keep the integrated test rather than duplicating its contract in a helper-only suite.
- **Production seam:** none is needed. The test starts the generated shell lifecycle stream through managed SSH and owns only the private fixture socket.
- The hosted connection target reports 8 passed and 1 failed; the complete job log is `/tmp/zmux-osc7501-final-6230bd4/universal-failed-job-full.log`. The failure line is `connection.rs:549`, with errno 35 `WouldBlock`. The parent-confirmed app build and native checks were skipped after the aggregate failure. Earlier local or hosted passes do not explain this later failure.
- No test was rerun here. The failed log records the outer test result, not the event deadline remaining when the valid frame was written, when `valid_receipt` processed it, or whether `sendto` succeeded. No root cause beyond the concrete test-side timeout mismatch is claimed.
