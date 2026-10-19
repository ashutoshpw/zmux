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
- [ ] Execute the implemented packaged smoke designed to traverse two fixture origins through
  the CryptoKit/client fixture, sharing host and Rust PTY for session listing, frames, input,
  resize, denial and recovery behavior; native execution awaits hosted macOS.
- [x] Run service database integration, portable runtime/ABI checks, hosted cloud checks and image build.
- [x] Complete final independent native source review with no blocking findings.
- [ ] Complete hosted macOS compilation, packaged execution and universal artifact; report
  physical two-Mac validation separately.
- [ ] Obtain separate serving-database activation approval before executing the rollout plan.

## Acceptance criteria
- [x] Sequential/concurrent registration returns one stable device for one installation; distinct
  installations sharing a name remain separate and revoked registrations fail closed.
- [x] Service checks cover account/device proof, replay, expiry, sharing epoch and queue bounds.
- [ ] The packaged two-origin CryptoKit/client-to-host-to-Rust-PTY smoke executes successfully;
  implementation exists, but native execution awaits hosted macOS. Separate Python dual-origin
  HTTP fixture/report checks and Rust PTY tests passed; neither is combined native evidence.
- [x] Restore handling stops advertising sharing after authoritative Rust grant loss; sharing-
  disable races and known rejected outcomes fail closed without silent replay or wrong-epoch cleanup.
- [x] Final native acceptance source review returned no blocking findings.
- [ ] Hosted macOS compile/packaged execution, universal artifact and physical two-Mac validation
  are complete.
- [x] Unrelated profile-lifecycle work and production service/database/tunnel state were preserved.

## References
- [references/protocol.md](references/protocol.md) — shared device, relay and terminal contract.
- [references/security-and-validation.md](references/security-and-validation.md) — authorization and acceptance boundaries.
- [references/rollout.md](references/rollout.md) — local migration, capability checks, public-route gate and rollback limits.
- [status.md](status.md) — discoveries, validation and remaining delivery gates.
