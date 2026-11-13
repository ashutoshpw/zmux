# Managed SSH hook-event receipt deadline fixture repair

**Date:** 2026-11-13 UTC

**Baseline:** `6230bd4fb6842d15901f1c02daab7fa91dfb9dea`

**Scope:** Repair the receipt timing contract in the existing loopback managed-SSH test. No production timeout, API, or test seam changed.

## Finding and accepted contract

The hosted universal run `43398619986`, job `131797064381`, failed at the valid-receipt socket read with macOS `WouldBlock`. The socket still had the 250 ms timeout installed for the wrong-generation negative control. That failure proves only that the fixture socket received no datagram during that short window; it does not establish a late acknowledgement or a production delivery defect.

The retained test is the strongest existing positive owner-boundary coverage: it runs the generated lifecycle stream through loopback SSH and checks receipt forwarding to the private per-hook socket. Its wrong-generation control must continue to prove no delivery. Keep every nonce, generation, event ID, sequence, disposition, and datagram assertion, and do not add a duplicate test.

## Repair

- The 250 ms socket wait is now installed only around the wrong-generation no-delivery assertion.
- The synthetic remote writer uses the same 1.8-second receipt budget as the production Claude hook's `RECEIPT_WAIT_SECONDS`.
- A local `Instant` deadline is anchored immediately before SSH writer execution. The valid receipt read receives only the time remaining to that deadline; the test fails if that budget expires before the positive read or its bounded socket wait times out.
- Failure diagnostics include local elapsed and remaining milliseconds, with no receipt identity values.

The remote Python owner starts its own `time.monotonic()` deadline when its writer runs. The Rust `Instant` and remote monotonic values have different origins and are never compared numerically. Both use the same duration, while the local anchor precedes SSH execution and therefore includes SSH setup time in the local allowance. This is a conservative client-side bound under the assumption that each host's monotonic elapsed duration represents comparable real elapsed time; it does not establish an exact cross-host deadline ordering or a synchronized clock relationship.

## Verification

- `cargo fmt --check --package zmux-remote-runtime` — passed.
- `cargo test -p zmux-remote-runtime --test connection managed_ssh_shell_routes_nonce_bound_opener_requests_from_loopback_sshd -- --exact --nocapture` — passed, 1 test; 8 filtered out; 1.09 seconds.
- `cargo clippy -p zmux-remote-runtime --test connection -- -D warnings` — passed.
- Scoped `git diff --check` — passed.

These are local focused checks. The passing loopback test exercises the real managed-SSH stream boundary, but does not reproduce the hosted schedule or prove the cause of the original failure beyond the fixture's inherited 250 ms read timeout. No controlled-delay probe was run; it would add a separate synthetic timing experiment without strengthening this owner-boundary result.

Parent review removed a newly added post-receive wall-clock assertion. The
socket wait already uses the remaining deadline; preemption after a successful
receive cannot independently establish late receipt delivery. All original
identity and no-delivery assertions remain. Focused/aggregate validation after
this small review correction remains with the parent.
