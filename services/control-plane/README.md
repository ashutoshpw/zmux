# zmux control plane

This is the self-hostable identity and device-control service for zmux. It uses Better
Auth 1.7.8 with PostgreSQL for browser email/password sessions, authenticator-based two-factor authentication and organizations. Its
native flow is an authorization-code flow with S256 PKCE for the single registered
development client `zmux-macos-dev` and exact callback `zmux-dev://oauth/callback`.

The service stores only hashes of authorization codes and native bearer tokens. Codes
expire after five minutes and are single-use. Native tokens expire after 30 days and are
bound to the Better Auth browser session that approved them; deleting or revoking that
session immediately invalidates its native tokens. Device records are owner-scoped.
Browser session cookies use `Secure` for every configured HTTPS origin, including
development servers behind a tunnel. Plain HTTP cookies are limited to loopback development.

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

## Two-factor authentication

The pinned Better Auth two-factor owner handles TOTP enrollment and verification,
password-login challenges, encrypted authenticator secrets and one-use recovery codes.
Enrollment requires the current password and a valid authenticator code before enabling
protection. A pending second-factor challenge is not a browser session and cannot authorize
a native client. Recovery codes should be saved privately; each successful use consumes one.
Email/SMS verification is not configured.

Run migrations before starting this version: the auth migration adds `twoFactor` and
`user.twoFactorEnabled`. Readiness rejects the old password-only schema. The local backup
restore rehearsal also requires the MFA table. Existing browser/native credentials are
not globally revoked by deploying this feature; ordinary session revocation still applies.

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
- `GET|POST /v1/devices`, `POST /v1/devices/:id/heartbeat`,
  `POST /v1/devices/:id/presence`, and `DELETE /v1/devices/:id` implement the
  owner-scoped registry. New registrations require `installation_id` (UUID),
  `device_secret` (32 random bytes as unpadded base64url), and `public_key` (32-byte
  X25519 public key). The same account, installation, proof and key update one row;
  changing the proof or key is rejected. Historical rows remain separate and only those
  rows can use the legacy bearer-only heartbeat route. Device listing adds the optional
  stable identity fields plus `online`, `sharing`, and `sharing_epoch`.
- Before registering, native clients must see
  `capabilities.stable_device_identity: true` from `GET /v1/config`; terminal clients
  also require `capabilities.terminal_relay: true`. The control plane advertises these
  after migration `003_device_terminal_relay.sql`, its device columns/index and relay
  table are present. Revoking a stable registration permanently reserves that installation
  identity; re-enrollment or recovery under the same identity is not supported yet.
- `POST /v1/devices/:id/presence` accepts `{ "sharing": boolean, "epoch": UUID }` and
  requires the registered device proof headers. Sharing is opt-in. A new grant must use a
  fresh epoch; an active grant rejects other epochs, while a fresh epoch may replace a
  grant whose 15-second presence lease has expired after a host restart.
- The encrypted relay uses `POST /v1/terminal-relay/requests`,
  `GET /v1/terminal-relay/inbox`, `POST /v1/terminal-relay/requests/:id/begin`,
  `POST /v1/terminal-relay/requests/:id/response`, and
  `GET /v1/terminal-relay/requests/:id`. Every relay call requires the original valid
  OAuth bearer token plus `X-Zmux-Device-Id` and `X-Zmux-Device-Secret`. The service
  stores opaque encrypted payloads, claims each request once, and rechecks the sender's
  token/session and the target's current sharing grant before begin. An executing request
  can finish after sender revocation while the target proof, grant and expiry remain valid.
  Status reads require the original sender device and original still-valid token.
- Relay JSON bodies are capped at 384 KiB, envelopes at 350,000 base64url characters,
  and payloads at 256 KiB plus envelope overhead. Requests expire after 20 seconds; at
  most 16 active requests per device, 128 per account and 512 retained rows per account
  are admitted. The frequent device-authenticated routes have a bounded per-device,
  per-route limit of 2,000 requests per minute.

Non-relay JSON endpoints use a 16 KiB body limit. Encrypted terminal-relay JSON bodies
allow up to 384 KiB; the Bun server enforces that as the overall request-body ceiling.
Browser auth and state-changing API calls enforce same-origin policy. Better Auth and
native endpoints use bounded per-process rate limits. A multi-replica production
deployment should add an edge rate limit because each process intentionally owns its
local counters.

## Production image and configuration

For the local Docker service and Cloudflare Tunnel handoff, use
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
`https://zmux.w3api.dev`; DNS, TLS, and hosting are deliberately not provisioned here.
Production rejects HTTP origins. PostgreSQL TLS certificate validation is enabled in
production, so the connection endpoint must present a certificate trusted by the image.
Set `TRUST_PROXY=true` only behind a trusted proxy that replaces `X-Forwarded-For`.
`HOST` accepts a literal IPv4 or IPv6 bind address and defaults to `0.0.0.0`; the local
Docker setup sets `HOST=127.0.0.1` explicitly.

For future hosted rollouts, run migrations as a protected GitHub Actions deployment step
before starting a new image. The local script backs up and migrates the persistent local
database; activation still requires separate approval.
Do not run them in image build or server startup. The runtime user is non-root, listens on
`PORT` (default 3000), and the image health check calls `/health`. Route external traffic
through an HTTPS reverse proxy to the same origin configured in `ZMUX_CLOUD_ORIGIN`.

After migrations, operators can run `bun run cleanup` with the same private service
environment. Each invocation removes at most 1,000 expired native authorization codes
and 1,000 expired native access tokens in one transaction, with a five-second statement
deadline. Concurrently locked rows are skipped. The JSON result contains deletion counts;
zero does not prove that another transaction holds no expired rows. Repeat later to drain
a backlog. Live credentials, browser sessions, accounts and devices are retained. This
command has no automatic schedule and does not implement broader audit/device retention.

VM provisioning, production tunnel ownership and verification, a standalone terminal host/daemon,
transactional email delivery, invite/recovery mail journeys, billing, public hosting,
DNS, TLS ownership and production observability remain open. The control-plane terminal
relay is implemented as opt-in encrypted RPC through the database to a currently running,
online macOS app; it does not allocate a VM or start a terminal daemon. Packaged native
end-to-end proof remains pending. Authenticator MFA and one-use recovery codes are
implemented as described above. The local Docker workflow includes database backup and
disposable restore verification; public deployment and callback acceptance remain
separate gates. `/ready` reports stable device identity and terminal relay as ready only
after migration 003 and its schema checks pass; VM and email remain unavailable.

### Native credential transactions

Code exchange, token revocation and expired credential cleanup share one transaction
owner. Statements are limited to five seconds and lock waits to three seconds. Failed
operations roll back; a failed rollback discards the physical pool client and retains
both failures. Asynchronous connection errors stay owned through client release.
These are per-statement server limits, not an overall network/request deadline.

`test/transactions.test.ts` exercises actual PostgreSQL commit/rollback, statement
cancellation and forced backend termination with a one-slot pool. It uses the same
throwaway database required by the HTTP integration suite; never run these tests against
a serving database.
