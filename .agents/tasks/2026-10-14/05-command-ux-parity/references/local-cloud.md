# Local cloud service

The user deferred hosted Actions deployment and authorized a local Docker/PM2 service.
Docker now runs the control plane on loopback port 3530, derived by `bunx path-to-port`
in `services/control-plane`. PostgreSQL uses a dedicated persistent volume and loopback
3531 with validated TLS. Tunnel target: `http://127.0.0.1:3530`; public origin remains
`https://zmux.w3dev.app`. No tunnel or public authentication verification is claimed.

`scripts/local-cloud` owns start/check/status/backup/stop. Private credentials, TLS and
logs stay outside Git. Startup backs up before migration; stop retains the volume.
Host networking avoids the existing bridge firewall restriction without firewall changes.
See `docs/cloud-deployment.md` for operation and certificate renewal.

## Verification

- Parent checked live `/health` and database-backed `/ready`: HTTP 200. Identity,
  devices and organizations ready; VM, relay and email unavailable.
- Parent service typecheck, migrations and integration tests passed against a newly
  created disposable database, which was dropped afterward. Persistent app DB untouched.
- `scripts/check` passed on an isolated archive of 85e6e9f plus cloud changes, avoiding
  concurrent persistence edits. This includes Rust tests, Clippy, notices and C ABI.
- Fresh gpt-6.1-sol review found inherited environment could override Compose's private
  DATABASE_URL and redirect migrations. Fixed by removing private-file keys from the
  Compose subprocess environment. Real Compose configuration regression passes; a
  negative control restoring inheritance fails at the database URL assertion.
- Fresh review accepted the repair; no remaining blocking findings in this scope.
- `scripts/test-local-cloud` never contacts a daemon or database.

## Open gates

Public tunnel/auth callbacks, restore rehearsal, off-host backups, scheduled credential maintenance,
VM/relay/email behavior and broader cloud parity remain open. Local readiness is not
full cloud parity.

## H01 expired credential maintenance

The operator-invoked cleanup command removes expired native authorization codes
and access tokens. One transaction deletes at most 1,000 expired rows per table with
row locking that skips concurrent consumers; live credentials and device/account records
remain untouched. No background scheduler or live database cleanup is part of this slice.
The command publishes deletion counts only.

Implementation adds `bun run cleanup` and an access-token expiry index. Typecheck and
all 12 real PostgreSQL integration tests pass against a fresh disposable database,
dropped afterward. The cleanup test inserts 1,002 expired credentials per table, holds
one row lock in each, verifies 1,000/1/0 batches, releases the locks, and verifies 1/0.
The retained live access token authenticates and the retained code exchanges normally.
No migration or cleanup was run against the persistent local service database.

Fresh independent review accepted the transaction, skip-locked admission, counts-only
operator output and retained-live-credential coverage. Broader H01/V01 remain open.

Hosted receipt: Cloud service checks38760632159 atd22b6a4 passed migration002,
service typecheck and all12 PostgreSQL integration tests, including the cleanup case.
This is hosted test evidence, not migration or maintenance of the running local service.

A public `/health` probe on2026-10-15 failed DNS resolution for `zmux.w3dev.app`
from this environment; no public service response or sign-in was observed. The local
loopback service still answers health/readiness and `bunx path-to-port` returns3530.
