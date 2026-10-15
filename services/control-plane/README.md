# Zmux control plane

This is the self-hostable identity and device-control service for Zmux. It uses Better
Auth 1.7.8 with PostgreSQL for browser email/password sessions and organizations. Its
native flow is an authorization-code flow with S256 PKCE for the single registered
development client `zmux-macos-dev` and exact callback `zmux-dev://oauth/callback`.

The service stores only hashes of authorization codes and native bearer tokens. Codes
expire after five minutes and are single-use. Native tokens expire after 30 days and are
bound to the Better Auth browser session that approved them; deleting or revoking that
session immediately invalidates its native tokens. Device records are owner-scoped.

## Local setup

Requirements: Bun 1.4.2, Docker, and an unused local PostgreSQL port. The example below
uses port 55437 and removes the database container when stopped.

```sh
docker run --rm --name zmux-control-postgres \
  -e POSTGRES_USER=zmux -e POSTGRES_PASSWORD=local-only \
  -e POSTGRES_DB=zmux -p 127.0.0.1:55437:5432 postgres:18-alpine
```

In another terminal:

```sh
cd services/control-plane
cp .env.example .env
# Set DATABASE_URL to postgres://zmux:local-only@127.0.0.1:55437/zmux.
# Generate BETTER_AUTH_SECRET with: openssl rand -base64 32
set -a; . ./.env; set +a
bun install --frozen-lockfile
bun run migrate
bun run dev
```

Migrations are explicit and protected by a PostgreSQL advisory lock. They do not run on
server startup. `bun run migrate` first applies the pinned Better Auth schema, then the
versioned SQL in `migrations/`; it is safe to run again. Stop the foreground Docker command
with Ctrl-C to remove the disposable container.

For integration tests, use a disposable migrated database:

```sh
set -a; . ./.env; set +a
bun run migrate
ZMUX_TEST_ORIGIN=http://127.0.0.1:41992 NODE_ENV=test bun run check
```

The tests truncate their configured database. Never point test commands at staging or
production. `../../scripts/check-cloud` runs the locked install, migration, service checks,
and production image build used by CI.

## HTTP contract

- `GET /health` means the process can serve HTTP. It does not query dependencies.
- `GET /ready` verifies PostgreSQL and reports each implemented or unavailable capability.
- `GET /v1/config` publishes the native client, callback, endpoints, and capabilities.
- `/api/auth/*` is Better Auth's browser API, including its organization/member endpoints.
- `GET /v1/native/authorize` requires the exact client/callback and S256 PKCE. It sends an
  unauthenticated browser to the sign-in page and sends an authenticated browser to the
  registered callback with an authorization code and the unchanged `state`.
- `POST /v1/native/token`, `POST /v1/native/revoke`, and `GET /v1/me` implement native
  token exchange, revocation, and identity.
- `GET|POST /v1/devices`, `POST /v1/devices/:id/heartbeat`, and
  `DELETE /v1/devices/:id` implement the owner-scoped registry.

JSON bodies are capped at 16 KiB. Browser auth and state-changing API calls enforce
same-origin policy. Better Auth and native endpoints use bounded per-process rate limits.
A multi-replica production deployment should add an edge rate limit because each process
intentionally owns its local counters.

## Production image and configuration

For the authorized local Docker service and Cloudflare Tunnel handoff, use
[`scripts/local-cloud`](../../scripts/local-cloud) as described in
[cloud deployment](../../docs/cloud-deployment.md#local-docker-service-and-tunnel-handoff).
It derives the port with `bunx path-to-port`, preserves production PostgreSQL TLS,
keeps credentials outside Git, and backs up before migrations.

Build and inspect the hosting-neutral image:

```sh
docker build -t zmux-control-plane:local services/control-plane
docker run --rm --env-file /secure/path/zmux-control.env \
  zmux-control-plane:local bun run migrate
docker run --rm --env-file /secure/path/zmux-control.env \
  -p 127.0.0.1:3000:3000 zmux-control-plane:local
```

Required production values are `DATABASE_URL`, `BETTER_AUTH_SECRET` (at least 32 random
bytes), and `ZMUX_CLOUD_ORIGIN`. The selected official origin is
`https://zmux.w3dev.app`; DNS, TLS, and hosting are deliberately not provisioned here.
Production rejects HTTP origins. PostgreSQL TLS certificate validation is enabled in
production, so the connection endpoint must present a certificate trusted by the image.
Set `TRUST_PROXY=true` only behind a trusted proxy that replaces `X-Forwarded-For`.
`HOST` accepts a literal IPv4 or IPv6 bind address and defaults to `0.0.0.0`; the local
Docker setup sets `HOST=127.0.0.1` explicitly.

For future hosted rollouts, run migrations as a protected GitHub Actions deployment step
before starting a new image. The authorized local script backs up and migrates locally.
Do not run them in image build or server startup. The runtime user is non-root, listens on
`PORT` (default 3000), and the image health check calls `/health`. Route external traffic
through an HTTPS reverse proxy to the same origin configured in `ZMUX_CLOUD_ORIGIN`.

VM provisioning, relay/tunnel service, transactional email delivery, MFA, invite/recovery
mail journeys, billing, hosting, DNS, TLS certificates, backups, observability, and a
production deployment are not implemented by this slice. `/ready` reports VM, relay, and
email capabilities as unavailable rather than presenting placeholder readiness.
