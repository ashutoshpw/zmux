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

## Persistent local service refresh — 2026-10-15 17:49 UTC

At committed `58a882b`, `scripts/local-cloud start` rebuilt the unchanged committed
control-plane source, completed its private pre-migration PostgreSQL backup, applied
migrations and recreated the app. Both `/health` and database-backed `/ready` return
HTTP200. A direct schema query confirms `001_native_identity.sql`,
`002_native_token_expiry.sql` and `native_access_token_expiry_idx` in the persistent DB.
An operator-invoked bounded cleanup completed with `codes:0,tokens:0`. No periodic
schedule was installed. After readiness, `bunx path-to-port` in the service directory
again returned3530. Tunnel target remains `http://127.0.0.1:3530`. Public tunnel and
auth callbacks remain unverified; VM, relay and email remain unavailable.

This receipt supersedes the earlier statements that migration002 and cleanup had
not been run against the persistent local service. Private credentials, backups and
operation logs remain outside Git under the existing local state directory.

## Local archive restore rehearsal — 2026-10-15

`scripts/local-cloud restore-check` now makes a fresh private archive and restores it
with `pg_restore --single-transaction --exit-on-error` into a fresh random database
on the owned PostgreSQL instance. It validates the eleven required tables, current
migration names and validated constraints, then drops only that temporary database.
The serving database is never replaced. Each attempt logs the temporary database name
privately for recovery if database creation or cleanup is interrupted. Cleanup failure
prevents a success receipt and retains both errors when restoration also failed.

Actual local execution restored a 22,537-byte archive with both migrations and no
unvalidated constraints; the disposable database was removed. The private receipt
contains only the archive hash, schema names and verification scope. The real PostgreSQL
failure-path check rejects an invalid archive and a transport failure after actual database
creation, verifies the database inventory is unchanged, and does not replace the earlier
success receipt. An isolated checkout is rejected before adopting the running Docker
project. Compose environment isolation also passes;
serving health and database readiness remain HTTP200 after the rehearsal.

This closes the local archive/schema restore rehearsal gate only. It does not establish
account/session recovery through the public tunnel, independent off-host backups, VM/relay
recovery or full cloud parity. Backups and logs remain in the private state directory.

Parent isolated `scripts/check` passes287 Rust tests, strict Clippy, notices and the C ABI.
The final script-only cleanup changes also pass actual PostgreSQL failure-path checks.
Interrupted creation is always a failure: attempted cleanup does not establish server
completion when the transport itself was interrupted; the private log records that limit.

## H01 transaction ownership — implementation and local evidence

Unify code exchange, token revocation and expired credential maintenance under one
transaction owner. Every owned transaction has a five-second statement bound and a
bounded lock wait. A failed rollback discards the physical client and preserves both
errors, instead of returning a potentially aborted connection to the pool. Invalid
expired authorization codes retain their existing committed deletion behavior.

Acceptance: real PostgreSQL commit/rollback and statement deadline; forced physical
connection termination must preserve the primary/rollback errors and allow a one-slot
pool to obtain a fresh usable backend. Existing HTTP auth/device/cleanup integration
checks must still pass. No test-only production configuration or mock database owner.

The disposable PostgreSQL suite now passes15 tests, including the three transaction
owner cases. The forced-backend termination case initially failed with an unhandled
`Connection terminated unexpectedly` event: the owner now captures asynchronous errors
until physical release, preserves both the primary/rollback failures, and the same
one-slot pool successfully obtains a different backend. The five-second statement
cancel test verifies SQLSTATE57014 and a usable subsequent query. Typecheck passes.
The persistent service database was not used for these tests. Production service refresh
and hosted current-head checks remain separate; limits are per statement, not a network
deadline or total transaction duration.

Independent source review accepted client ownership, actual pg-pool release/listener
ordering, committed invalid-code cleanup and the three distinct PostgreSQL test boundaries.
Parent isolated `scripts/check` also passes287 tests, strict Clippy, notices and C ABI.
Current macOS workflow shutdown-test failures are tracked separately in F04; this cloud
slice does not establish native or full-product acceptance.

At22:23UTC, `scripts/local-cloud start` backed up and rebuilt committed54a0e52,
recreated the local service and returned HTTP200 for health/readiness at3530.
Parent compared the running container's db/app/maintenance source hashes to the committed
files; all three match. This is local runtime promotion, not public tunnel/auth acceptance.
