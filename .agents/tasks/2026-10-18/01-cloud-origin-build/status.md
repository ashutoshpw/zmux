# Status

- **State:** in-progress
- **Updated:** 2026-10-18

## Evidence
- `RUST_TEST_THREADS=2 scripts/check` passed against an isolated HEAD export plus
  this task's source changes: formatting, workspace tests, Clippy, license inventory,
  native C ABI and profile-lease checks. Unrelated dirty runtime changes excluded.
- Compose configuration validates with placeholder credentials; production config
  parsing accepts the new HTTPS origin. Python scripts parse successfully.
- Baseline native saved-session validator rejects the actual v10 document logged by
  run 39070129651; corrected validator accepts it with reconstructed matching
  preparation metadata and continues to reject v9. Full journey requires macOS CI.
- Active source/config/docs contain no old hostname. Older task evidence is retained.
- Public curl `/health` at the new hostname returns 200. Public and loopback
  `/v1/config` still advertise the old origin before any service recreation.
  No authenticated browser/native login or live service reconfiguration is claimed.
- GitHub builds and artifacts pending publication of the scoped commit.
