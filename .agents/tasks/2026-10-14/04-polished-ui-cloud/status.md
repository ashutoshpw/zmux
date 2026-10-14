# Status

- **State:** in-progress (implementation and artifact delivered; production deployment deferred)
- **Updated:** 2026-10-14

## Delivered
- Polished lowercase native zmux UI, original icon, accessible symbol controls, active terminal titles and bundled CLI with shared Rust actions.
- Browser PKCE sign-in, Keychain credentials and account/device management; independent Better Auth/PostgreSQL service and Actions image packaging.
- Final native source `3585949a60845fbdcf4d76b3d49b2f62f970a104`: [download artifact](https://github.com/ashutoshpw/zmux/actions/runs/38637673581/artifacts/11928138411), expires 2026-10-28.
- Parent verified downloaded ZIP/checksum, universal app/CLI/library, permissions, icon/notices, matching source and all native smoke evidence; screenshot visually inspected.
- Local checks: 27 Rust tests, Clippy, formatting, notices, C ABI; cloud TypeScript, 11 real-PostgreSQL tests / 74 assertions and Docker build pass.
- Hosted Linux/macOS foundation, universal native compile/smoke, cloud checks and cloud package all pass. Fresh independent reviews returned `ship` with no findings.

## Deferred by user
- Production origin is `https://zmux.w3dev.app`. Hosting selection requires a separate discussion. All deployment must run through GitHub Actions; no live deployment is claimed.

## Remaining roadmap gates
- Remote PTY transport, VMs, relays, mail, MFA, billing and full PR #1 parity.
- Cloud retention cleanup and periodic native device heartbeat before production readiness.
- Manual native IME/accessibility, Intel execution, deployed sign-in, measured resource budgets, release signing/notarization.

See [implementation receipts](references/implementation.md) and [development builds](../../../../docs/development-builds.md).
