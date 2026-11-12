# Final route admission and acknowledgement repair

The independent budget review identified a ready-checkpoint path that could
resume after its caller deadline and add a route without a deliverable internal
acknowledgement. Parent added deadline/cancellation checks at command execution
and before acknowledgement, including rollback after mutation if expired.
Failed acknowledgement delivery removes only the exact unconfirmed reservation
and republishes its source state. Confirmed provider authority is preserved by
the existing cancellation owner. Ordinary commands and hook budgets are unchanged.

The existing private real-PTY regression now also exercises resumed checkpoint
commands with actual Session checkpoint results. Expired and cancelled envelopes
must return Timeout at the mutation owner. A dropped acknowledgement receiver
must leave a subsequent real OSC alert eligible. The stale-cache setup is
retained at the later live route admission; the direct resumed-command cases
advance the checkpoint cache as the production poller does. There is no new
production test seam or fake admission receipt.

Negative controls:

- Disabling the final expiry checks fails the expected Timeout assertion.
- Disabling failed-acknowledgement rollback suppresses the later real OSC alert:
  actual zero, expected one.
- Both controls were restored; the complete real-PTY regression passes.

Logs: `/tmp/zmux-osc7501-final-route-expiry-red.log` and
`/tmp/zmux-osc7501-final-route-ack-red.log`.

This verifies owner-side expiry and internal acknowledgement failure. It does
not claim that writing an external Unix-socket reply proves the launcher consumed
it, or establish the sole cause of the earlier hosted macOS failure. Fresh
hosted route execution remains required.
