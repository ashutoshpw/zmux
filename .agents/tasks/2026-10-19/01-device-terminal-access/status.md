# Status

- **State:** in-progress
- **Updated:** 2026-10-19

## Source and backend receipt

- Backend commit `06996d418e1cc0eff405de3a17b40f237a29a7af` was pushed and validated; backend
  paths have remained unchanged since that SHA. Hosted Cloud workflow `39225147711` succeeded
  on that SHA.
  Final `scripts/check-cloud` validation passed 35 tests and 423 assertions, and the control-
  plane image built. Parent log: `/tmp/zmux-device-terminal-cloud-final.log`.
- Backend review reproduced the P2 relay-claim lock inversion (`SQLSTATE 40P01`). The
  regression was fixed by taking the account lock before claiming. The focused relay suite
  passes 11 tests and 193 assertions; independent backend re-review found no remaining
  backend objection.
- The request-stream guard regression was reproduced as `204` for an over-limit body and
  fixed to return `413`, preserving the 16 KiB non-relay limit while relay bodies use their
  larger bound.

## Runtime and hosted native receipts

- Runtime commit `3ef82036082ce5e6a6629d5e2de31b8545e32600` records grant-scoped access to
  existing PTYs. `RUST_TEST_THREADS=2 scripts/check` passed, including clippy and C ABI checks.
  Rust execution rejects a stale queued sharing grant before input reaches the PTY. Rust/C ABI
  checks passed again in the final hosted macOS workflow; cloud validation remains the separate
  `06996d4` receipt above.
- Final macOS development workflow
  [`39238746402`](https://github.com/ashutoshpw/zmux/actions/runs/39238746402) and Foundation
  checks [`39238746676`](https://github.com/ashutoshpw/zmux/actions/runs/39238746676) succeeded
  on source SHA `633347ff33dce806ad99dc3e8662e0be306060c2`. The Mac workflow passed Swift checks
  for both architectures, Cloud credentials and encrypted relay, historical callback traps and
  the current callback path, Rust/C ABI checks, universal build and strict codesign verification,
  packaged lifecycle smoke, sidebar and status-menu checks.
- The packaged two-origin lifecycle report is `passed`; it exercised CloudClient, CryptoKit,
  CloudTerminalHost, NativeBridge and the Rust owner against two loopback origins and an existing
  shell. It records stable repeated registration, one listed session and frame, observed input,
  viewport and kernel resize at 18x72, `sharing_disabled` rejection, cleanup races and
  expired-session/account-switch gate checks. The broader native smoke report has `success: true`
  and stages `assertions_complete` on the same source SHA.
- Universal artifact [`12181162700`](https://github.com/ashutoshpw/zmux/actions/runs/39238746402/artifacts/12181162700)
  expires `2026-11-02T11:01:28Z`. The downloaded archive
  `zmux-dev-universal-633347f.zip` has SHA-256
  `2bf2fe16965952bebe32106bee1b444e9e4b355b816cc132fd640e8ce444ce4e`; independent verification
  confirmed outer and embedded build info source SHA `633347ff33dce806ad99dc3e8662e0be306060c2`,
  origin `https://zmux.w3api.dev`, and `arm64` plus `x86_64` headers for the app, CLI and native
  bridge library. Strict codesign verification passed; signatures are ad-hoc and the archive
  is not notarized. Receipt: `/tmp/zmux-device-final-633347f/artifact-verification.json`.
- Earlier hosted attempts exposed compile and callback-probe wiring issues; those were repaired.
  Focused transport run `39229640535` passed on `b4105c3`, and final independent native source
  review returned no blocking findings. The report-key mismatch from run `39235177700` was fixed
  in source SHA `633347f`; the corrected validator passed against the real report, recorded in
  `/tmp/zmux-device-native-c0a1c08-diagnostics/validator-key-red-green.log`.

## Approved serving activation

- After explicit user approval, `scripts/local-cloud start` and `scripts/local-cloud check`
  completed successfully on 2026-10-19. The control-plane service and persistent serving database
  were activated; this is recorded separately from the hosted macOS artifact workflow.
- Pre-migration archive: `~/.local/state/zmux/control-plane/backups/pre-migration-20261019T113401997337Z.dump`,
  25,929 bytes, SHA-256
  `245b0474c74be032c7ce01800c7b24c865b813b9dfd2ca80945ee34a9945b63c`. Parent verified the
  file hash, size, mode `0600`, and PostgreSQL custom-dump header. Restore testing was not done.
- A read-only serving-database ledger query showed `001_native_identity.sql`,
  `002_native_token_expiry.sql`, and `003_device_terminal_relay.sql`. The running app image has
  SHA-256 `b10a718017f5eaf5fe2d1cc659c683e4bd1395af5b60a60c652ea7f6f01f9150` and was healthy;
  the app container started at `2026-10-19T11:34:09.581537922Z`. The owned PostgreSQL container
  was healthy and started at `2026-10-19T11:34:04.646782212Z`.
- At approximately 11:34–11:35 UTC, direct local checks against `http://127.0.0.1:3530` and
  public checks against `https://zmux.w3api.dev` returned HTTP 200 for `/health`, `/ready` and
  `/v1/config`. Both origins reported healthy/ready status, `stable_device_identity` and
  `terminal_relay` ready/true, the configured public origin, and `vm`/`email` false. The existing
  public route was reachable with a normal Mozilla user agent. No DNS or tunnel changes were made.
- The script's `public_tunnel_verified: false` means it did not probe the route; the direct public
  checks above independently verified endpoint readiness. Physical two-Mac behavior and an
  authenticated terminal-client journey remain unverified. The user's running cmux app and hosted
  infrastructure were not changed.

## Remaining gates

- Complete physical two-Mac and authenticated terminal-client validation. Public health and
  capability endpoints are verified, but they do not prove authenticated client attachment or
  remote terminal interaction. The source-pinned artifact is verified and is not notarized.
- Future serving-database or service mutations require separate approval. The 2026-10-19
  activation was explicitly approved and completed; see [activation receipt](references/activation-receipt.md).
