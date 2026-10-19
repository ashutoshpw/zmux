# Local rollout and rollback plan

This is an operator plan only. No database, local service, public origin, or tunnel was
changed while writing it. `scripts/local-cloud` is the documented local path, but activation
against the persistent serving database requires separate approval. This plan does not
authorize that operation or constitute a production deployment workflow.

The parent's read-only pre-rollout snapshot on 2026-10-19 found
`http://127.0.0.1:3530/v1/config` reporting the legacy keys `identity: true`,
`devices: true`, and `relay: false`; that response had no `stable_device_identity` or
`terminal_relay` field. The external urllib probe returned HTTP 403 with Cloudflare error
1010 (browser integrity), so it did not verify the service route. These observations are
pre-migration evidence; they neither establish that a public mapping is absent nor replace
the post-migration checks below.

## Promotion sequence

1. Review the exact checkout and migration `003_device_terminal_relay.sql`. It adds nullable
   installation identity fields, sharing fields with a disabled default, an account-scoped
   unique index, and the encrypted relay table. Historical rows keep null installation
   identity and remain separate; the migration does not merge them by name.
2. After separate approval to activate the persistent serving database, run
   `scripts/local-cloud start` from the repository root.
   The script builds the service image, starts the owned PostgreSQL container, saves a private
   custom-format `pg_dump` under `~/.local/state/zmux/control-plane/backups/` by default
   (or under `ZMUX_LOCAL_STATE`), applies the explicit migrations (including 003), then
   recreates only the app container. It stops before promotion if backup or migration fails.
   The pre-migration archive is local and has no automatic retention or off-host copy.
3. Run `scripts/local-cloud check` and inspect its private `health.json` receipt. It checks
   `/health` and database-backed `/ready` on the local service. Then inspect all three local
   endpoints directly:

   ```sh
   curl --fail --silent --show-error http://127.0.0.1:3530/health
   curl --fail --silent --show-error http://127.0.0.1:3530/ready
   curl --fail --silent --show-error http://127.0.0.1:3530/v1/config
   ```

   Require `/health` to return `status: "ok"`; `/ready` must return `status: "ready"`,
   `stable_device_identity: "ready"` and `terminal_relay: "ready"`; `/v1/config` must report
   `cloud_origin: "https://zmux.w3api.dev"`, `stable_device_identity: true` and
   `terminal_relay: true`. Confirm that `vm` and `email` remain unavailable/false. A healthy
   process alone does not prove migration readiness.
4. `scripts/local-cloud start` does not create or validate a public route. First verify
   whether the owner has already mapped the origin; do not create or change a tunnel as part
   of this rollout plan. After confirming the existing route and an authorized external
   path, check the same endpoints through it:

   ```sh
   curl --fail --silent --show-error https://zmux.w3api.dev/health
   curl --fail --silent --show-error https://zmux.w3api.dev/ready
   curl --fail --silent --show-error https://zmux.w3api.dev/v1/config
   ```

   If these requests receive Cloudflare error 1010, that is a blocked integrity check, not
   a service health result; repeat the check through an authorized path.
   Require the same readiness and capability values, with the configured public origin in
   `cloud_origin`. Recheck from a permitted path that can complete any Cloudflare integrity
   check. The local receipt's `public_tunnel_verified: false` records that the script did not
   verify the public route.
5. Keep the upgraded client build and service change coordinated: current service
   `POST /v1/devices` requires `installation_id`, `device_secret`, and `public_key`. Older
   clients that send only name/platform receive `400 invalid_device`; only historical rows
   retain the legacy bearer-only heartbeat. An implemented packaged smoke is designed to
   traverse two fixture origins through the CryptoKit/client fixture, sharing host and Rust
   PTY for session listing, frames, input and resize. Native execution awaits hosted macOS; no
   combined CryptoKit-to-host-to-Rust-PTY run has occurred. Separate Python dual-origin HTTP
   fixture/report checks and Rust PTY tests passed, but do not establish that combined
   boundary. Hosted macOS compilation, packaged execution, the universal artifact and physical
   two-Mac validation remain gates.

## Rollback limits

- Migration 003 is additive, but the service has no migration-down command. Preserve the
  migration and rows when reverting app code; do not drop the new columns, index or relay
  table. `scripts/local-cloud restore-check` validates an archive in a disposable database
  and is not a serving-database rollback.
- Rolling back only the macOS client to an older version does not make the new API accept
  legacy registration bodies; those clients continue to receive `400 invalid_device`.
  Rolling the control-plane service back to its pre-change code restores legacy request
  acceptance and makes stable-identity capability checks fail for upgraded clients; it also
  restores per-POST device creation for old clients. Migration 003 can remain in place.
  Treat a service rollback as disabling the feature and coordinate client/service versions.
- If migration succeeds but app recreation or readiness fails, retain the pre-migration dump
  and migrated database for diagnosis. `scripts/local-cloud stop` stops only the owned local
  containers and retains the database volume. Restoring the dump over the serving database
  would discard registrations and relay records written since the backup and requires a
  separate reviewed recovery decision; this plan does not authorize that operation.
- Local backups are not off-host recovery. Any changes to public DNS, TLS, tunnel or external
  routing, plus authenticated public client checks, remain separate owner actions.
