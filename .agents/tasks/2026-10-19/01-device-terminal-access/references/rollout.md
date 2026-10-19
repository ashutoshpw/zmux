# Local rollout and rollback plan

This operator plan now includes the receipt for the explicitly approved 2026-10-19 activation.
`scripts/local-cloud start` and `scripts/local-cloud check` completed successfully; the serving
database was migrated and the local control-plane service restarted. See
[activation receipt](activation-receipt.md) for the archive, migration ledger, running image and
endpoint evidence. This record does not authorize future database or service mutations and does
not constitute a hosted production deployment workflow.

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
2. **Completed 2026-10-19 after separate user approval:** run `scripts/local-cloud start` from
   the repository root.
   The script builds the service image, starts the owned PostgreSQL container, saves a private
   custom-format `pg_dump` under `~/.local/state/zmux/control-plane/backups/` by default
   (or under `ZMUX_LOCAL_STATE`), applies the explicit migrations (including 003), then
   recreates only the app container. It stops before promotion if backup or migration fails.
   The pre-migration archive is local and has no automatic retention or off-host copy. Its
   verified name, size, hash and file mode are in the activation receipt; it was not restore-tested.
3. **Completed after activation:** `scripts/local-cloud check` and direct local endpoint checks
   returned HTTP 200 for `/health`, `/ready` and `/v1/config`. It checks
   `/health` and database-backed `/ready` on the local service and records them in private
   `health.json`; the direct checks covered all three endpoints:

   ```sh
   curl --fail --silent --show-error http://127.0.0.1:3530/health
   curl --fail --silent --show-error http://127.0.0.1:3530/ready
   curl --fail --silent --show-error http://127.0.0.1:3530/v1/config
   ```

   The observed `/health` returned `status: "ok"`; `/ready` returned `status: "ready"`,
   `stable_device_identity: "ready"` and `terminal_relay: "ready"`; `/v1/config` reported
   `cloud_origin: "https://zmux.w3api.dev"`, `stable_device_identity: true` and
   `terminal_relay: true`; `vm` and `email` remained unavailable/false. A healthy
   process alone does not prove migration readiness.
4. **Completed 2026-10-19:** the existing public route was reachable; no DNS or tunnel changes
   were made. Direct checks through `https://zmux.w3api.dev` returned HTTP 200 for the same
   endpoints; the public probe used a normal Mozilla user agent:

   ```sh
   curl --user-agent 'Mozilla/5.0' --fail --silent --show-error https://zmux.w3api.dev/health
   curl --user-agent 'Mozilla/5.0' --fail --silent --show-error https://zmux.w3api.dev/ready
   curl --user-agent 'Mozilla/5.0' --fail --silent --show-error https://zmux.w3api.dev/v1/config
   ```

   The public endpoints reported the same readiness and capability values, with the configured
   origin in `cloud_origin`; `vm` and `email` remained unavailable/false. A prior pre-activation
   urllib probe received Cloudflare error 1010 and did not verify service health. The later
   direct Mozilla-user-agent checks established public endpoint readiness. The local receipt's
   `public_tunnel_verified: false` still records that the script itself did not probe the route.
5. Keep the upgraded client build and service change coordinated: current service
   `POST /v1/devices` requires `installation_id`, `device_secret`, and `public_key`. Older
   clients that send only name/platform receive `400 invalid_device`; only historical rows
   retain the legacy bearer-only heartbeat. The final hosted macOS workflow
   [`39238746402`](https://github.com/ashutoshpw/zmux/actions/runs/39238746402) succeeded on
   source SHA `633347ff33dce806ad99dc3e8662e0be306060c2`; its corrected lifecycle report passed
   the two-origin CloudClient/CryptoKit, sharing-host, NativeBridge and Rust PTY path for
   registration, session listing, frames, input, resize, denial, cleanup races and account-switch
   recovery. Universal artifact
   [`12181162700`](https://github.com/ashutoshpw/zmux/actions/runs/39238746402/artifacts/12181162700)
   (expires `2026-11-02T11:01:28Z`) was independently verified against that source SHA and has SHA-256
   `2bf2fe16965952bebe32106bee1b444e9e4b355b816cc132fd640e8ce444ce4e`; it carries ad-hoc
   signatures and is not notarized. Physical two-Mac and authenticated terminal-client validation
   remain open; public endpoint readiness does not prove a client can attach or exchange input.
   See [activation receipt](activation-receipt.md) for the serving-state and endpoint evidence.

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
