# Registered device identity and terminal access

**Date:** 2026-10-19
**Task:** 01-device-terminal-access

## Objective
Repeated registration from one installation updates one account-owned device. Registered
online Macs can list and attach to explicitly shared terminal sessions, send input and
resize, and reconnect without replaying input. Deliver validated source and a native build.

## Context
- User reported duplicate registration and authorized both fixes together.
- At task start, registration inserted a new UUID for every POST and native sent only
  name/platform; the service had identity/device APIs but no terminal transport.
- Local SSH remains a separate feature from this account-scoped cloud terminal path.
- Preserve unrelated profile-lifecycle work and cloud-origin status edits.
- Original MIT implementation only; reuse Zmux session owners, no cmux implementation.

## Approach
Use a per-installation Keychain identity, device proof, and encryption key. Enforce account-
scoped uniqueness and reject key substitution. Do not infer physical identity from names
or silently merge historical rows. Add a bounded database relay for encrypted terminal RPC
between authenticated same-account devices, with short expiry, single claim, revocation
checks, explicit host sharing and honest unknown outcomes. Host sessions remain owned by
the running app. Reuse Rust PTY operations and native terminal rendering.

## Steps
- [x] Inspect backend/native owners and agree one versioned protocol before edits.
- [x] Reproduce registration duplication; implement stable identity and database constraint.
- [x] Implement authenticated bounded relay, presence, native encryption/transport and sharing UI.
- [x] Execute the packaged two-origin smoke through CloudClient/CryptoKit, CloudTerminalHost,
  NativeBridge and the Rust PTY owner; final workflow `39238746402` reports success on source
  `633347ff33dce806ad99dc3e8662e0be306060c2`, with lifecycle state `passed` and kernel size
  `18 72`.
- [x] Run service database integration, portable runtime/ABI checks, hosted cloud checks and image build.
- [x] Complete final independent native source review with no blocking findings.
- [x] Complete the final hosted macOS workflow and independently verify the universal artifact
  for source `633347ff33dce806ad99dc3e8662e0be306060c2`; report physical two-Mac validation
  separately.
- [ ] Complete physical two-Mac and authenticated terminal-client validation. The existing
  public route's health and capability endpoints have been verified separately.
- [x] Obtain separate serving-database activation approval and execute the rollout; see
  [activation receipt](references/activation-receipt.md).

## Acceptance criteria
- [x] Sequential/concurrent registration returns one stable device for one installation; distinct
  installations sharing a name remain separate and revoked registrations fail closed.
- [x] Service checks cover account/device proof, replay, expiry, sharing epoch and queue bounds.
- [x] The packaged two-origin CryptoKit/client-to-host-to-Rust-PTY smoke report passes in hosted
  run `39238746402`, with the corrected checker and final workflow succeeding; this does not
  establish physical two-Mac or authenticated public-route acceptance.
- [x] Restore handling stops advertising sharing after authoritative Rust grant loss; sharing-
  disable races and known rejected outcomes fail closed without silent replay or wrong-epoch cleanup.
- [x] Final native acceptance source review returned no blocking findings.
- [x] The final hosted macOS workflow succeeds and its universal artifact is verified against
  source SHA `633347ff33dce806ad99dc3e8662e0be306060c2`.
- [x] After explicit approval, the serving database and local service were activated; local and
  existing public-route readiness/capability endpoints passed. See the activation receipt.
- [ ] Physical two-Mac and authenticated terminal-client validation are complete.
- [x] Unrelated profile-lifecycle work and public DNS/tunnel configuration were preserved.

## References
- [references/protocol.md](references/protocol.md) — shared device, relay and terminal contract.
- [references/security-and-validation.md](references/security-and-validation.md) — authorization and acceptance boundaries.
- [references/rollout.md](references/rollout.md) — local migration, capability checks, public-route gate and rollback limits.
- [references/activation-receipt.md](references/activation-receipt.md) — approved activation, backup, migration, image and endpoint evidence.
- [status.md](status.md) — discoveries, validation and remaining delivery gates.
