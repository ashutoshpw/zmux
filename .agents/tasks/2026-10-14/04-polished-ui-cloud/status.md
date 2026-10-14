# Status

- **State:** in-progress
- **Updated:** 2026-10-14

## Implemented
- Native workspace polish, original generated icon, terminal metadata, bundled automation CLI and native cloud account/device client committed in `5e76140`.
- Independent Better Auth/PostgreSQL service implements PKCE, session-bound credentials, owner-scoped devices and migrated-schema readiness.
- Parent reran cloud checks: 11 PostgreSQL tests, 74 assertions, TypeScript and Docker build pass.
- Rust tests/Clippy pass locally; C ABI exit regression is being investigated after a fresh failure.
- Hosted first pass found snapshot-read contention and macOS socket fixture path length; fixes underway before artifact delivery.

## Pending gates
- Fresh review, successful hosted native compile/smoke, downloaded artifact verification.
- Production origin is `https://zmux.w3dev.app`; host selection is explicitly deferred. All deployment must use GitHub Actions. No live service is claimed.
- Remote PTY transport, VMs, relays and broader roadmap acceptance remain unimplemented.
