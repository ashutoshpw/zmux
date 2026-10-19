# Serving database activation receipt

**Date:** 2026-10-19
**Observed:** approximately 11:34–11:35 UTC
**Task:** 01-device-terminal-access

## Approval and operation

The user separately approved activating the persistent serving database. On 2026-10-19,
`scripts/local-cloud start` completed successfully in approximately 13 seconds, and the separate
`scripts/local-cloud check` also succeeded. The operation applied the checked-in migration set
and restarted the local control-plane service against the serving database.

The pre-migration archive is
`~/.local/state/zmux/control-plane/backups/pre-migration-20261019T113401997337Z.dump`.
It is 25,929 bytes with SHA-256
`245b0474c74be032c7ce01800c7b24c865b813b9dfd2ca80945ee34a9945b63c`. Its mode is `0600`.
Independent read-only verification confirmed the size, hash, permissions and PostgreSQL custom-
dump (`PGDMP`) header. The archive was not restored or restore-tested.

A read-only serving-database ledger query returned:

- `001_native_identity.sql`
- `002_native_token_expiry.sql`
- `003_device_terminal_relay.sql`

The running app image has SHA-256
`b10a718017f5eaf5fe2d1cc659c683e4bd1395af5b60a60c652ea7f6f01f9150` and was healthy when
observed. Its container started at `2026-10-19T11:34:09.581537922Z`. The owned PostgreSQL
container was healthy and started at `2026-10-19T11:34:04.646782212Z`.

## Local and public endpoint checks

Direct checks against `http://127.0.0.1:3530` and the existing public origin
`https://zmux.w3api.dev` returned HTTP 200 for `/health`, `/ready` and `/v1/config`.
Both origins reported:

- `/health`: `status: "ok"`.
- `/ready`: `status: "ready"`, `stable_device_identity: "ready"`, and
  `terminal_relay: "ready"`.
- `/v1/config`: `cloud_origin: "https://zmux.w3api.dev"`,
  `stable_device_identity: true`, and `terminal_relay: true`; `vm` and `email` remained false.

The existing public route was reachable with a normal Mozilla user agent. No DNS or tunnel
configuration was changed. `scripts/local-cloud check` still reports
`public_tunnel_verified: false` because the script does not probe public routing; the direct
checks above were performed separately.

## Evidence boundary

This activation changed the persistent serving database and local control-plane service after
the recorded approval. It did not create hosted infrastructure or a hosted deployment pipeline,
change DNS or tunnel settings, or modify the user's running cmux app. Endpoint readiness does not
establish an authenticated remote terminal-client journey. Physical two-Mac behavior and the
authenticated terminal-client journey remain unverified. Future serving-database or service
mutations require separate approval.
