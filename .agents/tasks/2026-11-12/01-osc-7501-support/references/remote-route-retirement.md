# Remote provider route retirement

**Date:** 2026-11-12

**Scope:** accepted second P2 in [provider epoch recheck](provider-epoch-recheck.md), limited to desktop runtime route policy, its remote stream coordination, and owner tests.

## Cause

The remote `ProviderRoute::Starting` handler records a provider route before it queues the acceptance receipt. A successful local enqueue only means the bounded stream command queue accepted the receipt. The stream writer can later discard a receipt whose captured binding belongs to a replaced generation.

Transport retirement previously marked every route ended. `route_active` intentionally keeps ended invocation ownership so journal transitions that were already produced before provider exit are still arbitrated. That behavior also kept an unconfirmed reservation active after its provider had never received admission. An OSC alert could then select Provider ownership for that command and remain suppressed after the route disappeared.

## Repair

- Provider routes now carry confirmation separately from their ended phase. A route start creates an unconfirmed reservation. An accepted provider lifecycle observation creates confirmed provider ownership. A normal `Ended` phase confirms that the invocation was spawned before retaining its ended ownership. `Cancelled` does not confirm a route.
- Retiring an exact transport generation removes its unconfirmed reservations and releases an OSC source lock only when that lock was predicted by the exact removed reservation. Confirmed routes remain as ended owners for earlier journal transitions.
- Explicit cancellation and ordinary route pruning also release only their matching predictive locks. A provider alert promotes a predictive lock to established Provider ownership, so cancellation cannot erase authority already backed by an actual provider alert.
- Reservation identity keeps target, provider, route UUID, runtime ID, provider generation, transport runtime and generation, and session and command epochs. A transport-scoped lifecycle observation can establish Provider source authority for its exact transport owner, but does not rewrite the explicit pending route identity.
- `Stream::send_provider_route_receipt` now documents that `true` proves local queue admission only.

No wire change or Session producer change was needed.

## Regression evidence

The new policy regression was run before the repair:

```text
cargo test --locked --offline -p zmux-desktop-runtime --lib program_status::tests::retiring_an_unconfirmed_transport_route_releases_its_predictive_source_lock -- --exact
```

It failed at the intended assertion: after transport retirement, the later OSC alert count was `0`, expected `1`. The owner fix changed that result to one admitted alert.

The retained and added owner tests establish that:

- a retired unconfirmed reservation releases only its predictive source lock;
- a provider alert promotes a predictive lock before cancellation;
- a normally ended invocation remains an owner after transport retirement;
- explicit pre-spawn cancellation still admits a later OSC alert.

The SSH fixture regression uses the existing loopback SSH server and `Stream` owner. Its peer emits a real framed `ProviderRoute::Starting`, does not read the queued acceptance receipt, then sends a gapped sequence. The stream reports G1 loss and produces a G2 `Connected` with `previous_generation=G1`. The test retires the exact G1 transport and verifies the subsequent OSC alert is admitted.

## Checks

- Desktop Program Status policy tests: **14 passed**.
- Real SSH framed route and reconnect regression: **1 passed**.
- Existing remote route receipt binding test: **1 passed**.
- Existing remote provider-state and sequence-gap test: **1 passed**.
- Public Desktop real-PTY Program Status projection test: **1 passed**.
- Session runtime lifecycle and real-PTY Program Status tests: **17 + 6 passed**.
- `cargo clippy --locked --offline -p zmux-desktop-runtime --all-targets -- -D warnings`: **passed**.
- `cargo fmt -p zmux-desktop-runtime -- --check` and scoped `git diff --check`: **passed**.
- `scripts/check` was not run; the parent will run it once the concurrent fixes stabilize.

## Limits

The SSH regression proves a real route frame, receipt queue admission, stream loss and generation replacement, and later OSC admission. The peer leaves stdin unread, but the test does not pause the local writer at a deterministic point between queue acceptance and its first pipe write. The receipt may already have been written into the G1 SSH stdin buffer before loss. Forcing the precise writer-queue drop ordering would require a writer scheduling seam, which this change does not add.

There is also an unavoidable ambiguous-delivery window without a remote acknowledgement of the receipt itself. If G1 delivered the receipt and the provider child started, but no valid provider lifecycle observation or normal `Ended` phase reached Desktop before G1 loss, Desktop cannot distinguish that case from a receipt the provider never received. Recovery releases the unconfirmed reservation so a later OSC alert can proceed. Provider evidence or alerts already observed retain Provider authority and cadence. No stronger claim about an unobserved invocation is made.
