# Cloud deployment

The owned production origin is **https://zmux.w3api.dev**. All production cloud
deployments are postponed. The documented local setup runs in Docker; activating it
against the persistent serving database still requires separate approval.
The local setup neither creates nor verifies a public route. The owner may already have
mapped the hostname; inspect the existing route before making any tunnel or DNS changes.
The local setup alone does not claim that public deployment is complete.

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
**3530**. The tunnel target is **http://127.0.0.1:3530**. Check whether the owner already
maps the public hostname `zmux.w3api.dev` to that target before changing tunnel settings.
A tunnel agent in its own network namespace must reach the host loopback explicitly;
its container's `127.0.0.1` addresses that container. The app uses
`https://zmux.w3api.dev` for auth origins and secure cookies. External HTTPS, tunnel
access policy, and browser/native callback checks remain owner-controlled and require
independent validation.

Changing the hostname in Git does not update an already running container. Apply
the updated Compose app configuration when activating the new origin, then verify
that `/v1/config` reports `cloud_origin: https://zmux.w3api.dev` before testing login.

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
It records `public_tunnel_verified: false` because the script does not probe the public
route; that value does not establish whether a route already exists. After migration 003
and its schema checks pass, `/ready` reports identity, devices, organizations, stable
device identity and terminal relay ready; VM and email remain unavailable. The relay
carries encrypted requests through PostgreSQL to a currently running, online Mac that has
explicitly enabled sharing. It does not provision a VM or a separate terminal daemon.
Local readiness does not verify an existing public route or an authenticated public
browser/native flow.

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
configuration remain decisions for that discussion. Local Docker operations are separate
from hosted deployment; activation against the persistent serving database still requires
separate approval. No hosted Actions deployment workflow has been added.

The account/device service now provides an encrypted database relay for terminal requests
to explicitly shared, online Macs. The Mac app owns the terminal sessions. This does not
provision VMs or a standalone terminal daemon, and it does not verify any Cloudflare Tunnel
route. Keep the implemented capability states explicit in `/v1/config`, `/ready`, and the
native UI; the relay does not imply that a public route or remote VM is available.

## Remaining service operations

Before production rollout, add scheduled cleanup for expired authorization codes and
expired/revoked native tokens and device records, plus backups and monitoring. Device
presence uses an authenticated 15-second lease refreshed by the running app while terminal
sharing is active; registration alone does not establish that a Mac is currently online.

The current service requires stable installation identity fields on `POST /v1/devices`.
Older native clients that send only a device name and platform receive `400 invalid_device`
from the updated service. Keep updated clients and the service rollout coordinated; the
legacy bearer-only heartbeat remains limited to historical rows. See the device-terminal
task's [rollout plan](../.agents/tasks/2026-10-19/01-device-terminal-access/references/rollout.md)
for the local migration sequence, capability checks and rollback limits.
