# R03 V8 admitted-lifetime history and desktop consumption

Implementation record, 2026-10-16, in the isolated
`ci/execution-admission` worktree. The helper, admission protocol and actual
fast-exit behavior are owned by the remote-runtime slice. This record does not
claim native presentation, input readiness or platform activation.

## Persisted contract

The workspace writer now emits session document version 8. The completed terminal
variant stays `completed_local_zellij_terminal`, with the same profile/owner and
bounded captured viewport fields. Its receipt is the shared versioned
`TerminalReceipt`: legacy V1 retains its exact raw serialized shape, while V2 has
one explicit `execution_contract: admitted_lifetime_v2` envelope around its typed
receipt. V7 readers remain supported for legacy V1. A V2 receipt in a V7 document
is rejected, as are unsupported execution contracts and malformed/mixed receipt
fields. Reading a saved document never creates Run, ACK or retirement authority.

V8 restores either form as sessionless completed history, consumes no terminal
IDs, and emits no PTY, Start or Attach effects. Close/reopen preserves the complete
versioned receipt and its execution classification without replay. The immutable
captured workload viewport and its existing 16 KiB UTF-8 bound are unchanged.

## Desktop consumer

The native observer descriptor/frame/ACK wire revision is 3. It can carry legacy
V1 or explicit admitted-lifetime V2 evidence; the execution-contract version is
separate from this observer wire version. The native worker still independently
authenticates the exact original live publisher and receipt, with current
correlation, full server/supervisor identity and namespace checks. The separately
reviewed retained CLI pre-exec identity repair remains intact.

S01 uses the shared receipt accessors for owner generation and digest. Its actual
matching Saved result remains the only completion-authorization source; shutdown,
changed slot generation, failed or refused save still deny that authorization.
The desktop operation receipt preserves the raw retained Child exit, execution
contract version and optional execution observation. A correlated configured-shell
exec failure reports `execution_failed`, preserves its stage/errno/raw status,
and accepts CLI exit 1 as compatible with independently confirmed physical
cleanup. It does not masquerade as ordinary user exit 126/127 or as failed cleanup.
Normal V2 lifetime completion uses the ordinary mapped exit with its explicit
`not_observed` or `shell_observed` evidence. Neither observation proves native
input readiness or continuous executable-image integrity.

## Regression evidence and remaining gates

The independent raw V8 restore regression was committed before production repair
as `2d49a90`. Its exact command is:

```
cargo test -p zmux-workspace-core --test local_zellij v8_ -- --nocapture
```

The red receipt is
`/tmp/zmux-r03-v8-admission-restore-red-20261016/receipt.json`, SHA-256
`bd85012ebea33dc8f85dce30ed337f830bee22cc1d6b90d131041cb7e5a9dfcb`.
It fails because the legacy reader rejects the explicit V2 envelope. The fixture
uses independently precomputed domain-prefixed canonical binding digests,
including a recomputed digest for a deliberately split birth/lifetime tuple.
That negative cannot pass merely because its digest is stale.

Initial focused validation passed the core descriptor/session-document tests and
the real FIFO S01 tests. The S01 save table now distinguishes legacy V1, V2
not-observed completion and V2 exec failure. These synthetic terminal facts test
storage/correlation/classification; they are not live Run or publisher proof.
The protocol owner separately owns immediate exit 0/7, real startup-file exit,
helper gate, legitimate exec and failure-path real-process evidence. Source
freeze, final validation and independent source review are separate handoffs.

Swift V2 decoding/presentation is parent-owned. Exact synthetic receipt examples
and classification semantics are in
`/tmp/zmux-r03-v8-native-wire-examples-20261016.json`, SHA-256
`e74b533ce1bee32ea9305cd4d11be51fe26d2cd9fa4fb8515be2c6fc06048e6b`.
Keep the existing V1 native history fixture. Do not label the V2 native consumer
complete until the native model and its actual presentation checks are verified.
