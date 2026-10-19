# Status

- **State:** in-progress
- **Updated:** 2026-10-19

## Source and backend receipt

- The backend implementation is pushed at `06996d418e1cc0eff405de3a17b40f237a29a7af`,
  matching the remote exactly. Hosted Cloud workflow `39225147711` succeeded on that SHA.
  Final `scripts/check-cloud` validation passed 35 tests and 423 assertions, and the control-
  plane image built. Parent log: `/tmp/zmux-device-terminal-cloud-final.log`.
- Backend review reproduced the P2 relay-claim lock inversion (`SQLSTATE 40P01`). The
  regression was fixed by taking the account lock before claiming. The focused relay suite
  passes 11 tests and 193 assertions; independent backend re-review found no remaining
  backend objection.
- The request-stream guard regression was reproduced as `204` for an over-limit body and
  fixed to return `413`, preserving the 16 KiB non-relay limit while relay bodies use their
  larger bound.

## Runtime and native receipt

- Runtime commit `3ef82036082ce5e6a6629d5e2de31b8545e32600` records grant-scoped access to
  existing PTYs. `RUST_TEST_THREADS=2 scripts/check` passed, including clippy and C ABI checks.
  Parent log: `/tmp/zmux-device-terminal-runtime-final.log`. Rust terminal execution rejects a
  stale queued sharing grant before input reaches the PTY.
- Native identity, Keychain, encryption, transport, UI and packaged smoke implementation are
  present in source. The smoke is designed to traverse two fixture origins through the
  client/CryptoKit fixture, sharing host and an existing Rust PTY for session listing, frame
  delivery, split-marker input and resize (verify `stty size` reports `18 72`), plus denial and
  recovery cases. It has not been executed here; native execution awaits hosted macOS.
  Separately, Python AST/YAML checks and dual-origin HTTP fixture/report positive/negative checks
  passed, and the separate Rust PTY suite passed as part of `scripts/check`. There is no combined
  CryptoKit-to-host-to-Rust-PTY execution receipt.
- Review-driven repairs include strict Rust raw-JSON handling, canonical lowercase UUIDs,
  accurate error receipts, stopping share advertisement after restore loses the authoritative
  Rust grant, 4096-byte input and 16384-byte paste bounds, automatic resize consent, retained
  partial-cleanup retry on 503, and account-owned retirement only after Rust is off and the
  new account is proven. Unresolved old presence is surfaced. Known rejected 4xx outcomes,
  including 429, do not trigger wrong-epoch `sharing: false` cleanup.
- Final independent native acceptance review returned no blocking source findings. This is a
  source-review receipt only, not native execution evidence. This Linux environment has no
  Swift/Xcode, so native compilation, packaged execution and the universal artifact remain
  pending hosted macOS gates. Physical two-Mac behavior is unverified.

## Remaining gates

- Complete hosted macOS compilation, packaged execution and universal artifact validation;
  report physical two-Mac validation separately.
- The rollout plan is concrete and linked from `plan.md`. Activating the persistent serving
  database still requires separate approval; the plan does not grant it. No production
  database, service, tunnel or running app was changed for this task.
