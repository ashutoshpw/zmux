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

Public tunnel/auth callbacks, restore rehearsal, off-host backups, expired-token cleanup,
VM/relay/email behavior and broader cloud parity remain open. Local readiness is not
full cloud parity.
