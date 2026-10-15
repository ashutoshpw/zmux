# Cloud deployment

The owned production origin is **https://zmux.w3dev.app**. All production cloud
deployments are postponed. The current authorized setup runs locally in Docker;
the owner will map the origin with Cloudflare Tunnel after local health checks.
No public deployment or tunnel is claimed by the local setup.

The native app uses the validated origin in `config/deployment.json`. A running local
cloud service is useful for development and integration testing; it does not replace
production deployment.

## Available Actions workflows

- **Cloud checks** validates the service against disposable PostgreSQL, including native
  PKCE and authorization boundaries.
- **Cloud deployment package** is manually dispatched on `main`. It first calls the cloud
  checks, builds the container and uploads an immutable source-tagged image archive,
  checksum and intended-origin receipt. The receipt explicitly states `deployed: false`.

## Local Docker service and tunnel handoff

On this Linux host, run from the repository root:

```sh
scripts/local-cloud start
scripts/local-cloud check
scripts/local-cloud status
```

`start` runs `bunx path-to-port` in `services/control-plane`; the current result is
**3530**. The tunnel target is **http://127.0.0.1:3530**. Map the public hostname
`zmux.w3dev.app` to that target using a Cloudflare Tunnel agent on the same host.
A tunnel agent in its own network namespace must reach the host loopback explicitly;
its container's `127.0.0.1` addresses that container. The app uses
`https://zmux.w3dev.app` for auth origins and secure cookies. External HTTPS, tunnel
access policy, and browser/native callback checks remain owner setup and validation.

The `zmux-local-cloud` Compose project uses Linux host networking because this host's
outbound firewall blocks newly created Docker bridge subnets. Both services explicitly
bind loopback: Bun at `127.0.0.1:3530`, PostgreSQL at `127.0.0.1:3531`. It does not change
firewall rules or other Docker/PM2 services. The database has a dedicated persistent
`zmux-local-cloud_database` volume. Both containers restart unless manually stopped;
Docker must itself start at boot. The containers have CPU, memory and rotated-log limits.

Credentials, TLS keys, backup dumps and operation logs are stored outside Git at
`~/.local/state/zmux/control-plane/` (directory mode `0700`, private files `0600`).
The `local.env` file is generated once and reused. Keep it with the database volume;
losing it loses the generated database password and rotates authentication keys if
replaced. Do not paste `docker inspect`, `docker compose config`, or environment-file
contents into reports: they can contain secrets. `status` only displays container status.
`ZMUX_LOCAL_STATE` may select a different private directory outside the checkout.

PostgreSQL requires TLS for TCP connections and SCRAM password authentication. A local
CA signs a server certificate with the `127.0.0.1` SAN; Bun trusts only the mounted CA
certificate in addition to its normal trust store through `NODE_EXTRA_CA_CERTS`.
Production certificate validation stays enabled. The leaf certificate expires after
90 days and startup rejects certificates with less than a day remaining. Before expiry,
stop this project, preserve the private directory, remove its `tls/server.crt`, and run
`start` to regenerate its CA/leaf and recreate both containers. This pauses this local
service and does not change the database volume. Monitor certificate expiry and Docker
health while it is exposed through a tunnel.

Every start builds from the locked Bun dependency file, starts PostgreSQL, saves a private
`pg_dump --format=custom` backup, applies the explicit migrations, then starts the app.
A failed backup or migration stops promotion. Backups are local, contain database data,
and have no automated retention or off-host replication. Review migrations for backward
compatibility before restarting an existing service; there is no automatic database
rollback. Off-host recovery remains operational work.

```sh
scripts/local-cloud backup  # retains a private database archive
scripts/local-cloud restore-check # restores a fresh archive into a disposable database
scripts/local-cloud stop    # stops only this project; retains database and credentials
```

`restore-check` takes a fresh backup, restores it transactionally into a randomly named
empty database on the same PostgreSQL instance, verifies the migrated schema and validated
constraints, then drops that temporary database. It never replaces the serving database.
A private `restore-check.json` receipt records the archive hash and schema only, with no
account contents. A failed restore or cleanup returns failure; operation logs remain private.
This requires temporary disk space for a second database and is a local archive recovery
check, not an off-host recovery or browser authentication rehearsal. The development
failure-path check `scripts/test-local-cloud-restore` also uses only disposable databases.

`check` records `/health` and database-backed `/ready` results in private `health.json`.
It records public tunnel verification as false. Identity, devices and organizations may
be ready while VM, relay and email remain unavailable. Local readiness does not establish
an authenticated public browser/native flow.

## Host-selection handoff

If hosted deployment is resumed, extend the package workflow with a deployment job that:

1. Uses a protected `cloud-production` GitHub environment and provider-specific secrets.
2. Deploys the exact validated source/image, without mutable unverified image tags.
3. Injects PostgreSQL credentials and Better Auth secret from environment secrets.
4. Runs the service's migrations through the protected job before promotion.
5. Configures TLS and the owned origin without a fallback to another product's service.
6. Checks the live readiness endpoint and performs the authenticated native/browser journey.
7. Records image/source, origin, migration result and health evidence in the Actions receipt.

Remote host credentials, hosted database provisioning, public DNS/TLS and production mail
configuration remain decisions for that discussion. The current local Docker setup is
authorized separately; no hosted Actions deployment workflow has been added.

Account/device APIs do not yet provide remote PTY transport, VMs or relay services. Their
availability must remain explicit in both the service readiness response and native UI.

## Remaining service operations

Before production rollout, add scheduled cleanup for expired authorization codes and
expired/revoked native tokens and device records, plus backups and monitoring. Native
registration currently updates `last_seen_at` once; periodic native heartbeats are a
separate device-presence feature. The account/device UI does not imply a live connection.
