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
- A concurrent session committed and pushed the pre-existing terminal-limit work as
  `083ed31` during initial validation. Re-exported exact commit `b1c9589` and reran
  `RUST_TEST_THREADS=2 scripts/check` successfully before pushing.
- Scoped commit `b1c9589f807c9ca288183a54b3b771a68c209e95` pushed to `origin/main`;
  `git ls-remote` confirms exact parity. Remaining untracked work is untouched.
- Native run: https://github.com/ashutoshpw/zmux/actions/runs/39165166886
- Cloud image package run: https://github.com/ashutoshpw/zmux/actions/runs/39165174328
- Foundation run 39165166847 passed on Linux and macOS; cloud service run
  39165166795 and cloud image package run 39165174328 passed.
- Downloaded cloud artifact 12150840139 and verified its SHA-256 manifest and
  deployment receipt: exact implementation SHA, new origin, `deployed: false`.
- Native run 39165166886 passed both source slices, focused native checks,
  Rust/C ABI, universal app compilation and the actual saved-session close/reopen
  journey. GitHub canceled it during broad smoke after exceeding 25 minutes;
  check annotations confirm the timeout. No native test assertion failed first.
- Increased only the full development job limit from 25 to 45 minutes; all checks,
  artifact gates and focused suite limits remain unchanged. Native retry pending.
- The timeout-only workflow adjustment reuses the passing local/core validation;
  hosted native execution is the acceptance gate for this operational change.
- No live service deployment was performed.
