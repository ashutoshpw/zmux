# Status

- **State:** in-progress
- **Updated:** 2026-10-14

## Notes
- Implementation authorized. [Foundation task](../02-foundation-prototype/plan.md) tracks initial slices;
  [native development task](../03-macos-development-build/plan.md) tracks artifact delivery.
- Portable foundation complete: MIT setup, Rust workspace/action core, headless PTY/parser, CLI and C ABI feasibility bridge.
- `scripts/check` passes 27 Rust behavioral tests, C ABI checks, formatting, Clippy and dependency inventory.
- `main` is published to `https://github.com/ashutoshpw/zmux.git`; hosted Linux/macOS foundation checks pass.
- Native app implements live local shells, workspaces, splits, input and viewport rendering.
  Universal build, ad-hoc signing and packaged native smoke pass.
- [Development artifact](https://github.com/ashutoshpw/zmux/actions/runs/38637673581/artifacts/11928138411) delivered at `3585949`; receipt is in the UI/cloud task and development-build docs.
- M0 resource baselines and manual native UX checks remain open; partial M1/M2 behavior is implemented.
- Chosen architecture: Rust shared core/CLI/session services with thin Swift/AppKit macOS UI.
- egui/eframe is deferred. cmux remote-session hangs are unverified motivation for stress gates.
- PR #1 behavior remains required independently authored scope. Better Auth identity, organizations, native PKCE and owner-scoped device service landed; full parity is incomplete.
- Polished native UI, original icon, active terminal titles, bundled CLI and Keychain cloud client are included. Cloud image packaged through Actions, but production deployment is deferred pending hosting selection.
- Working defaults: Ashutosh Kumar and Zmux contributors for copyright, macOS 14+ target,
  Rust 1.95, Linux-first remote host support, compatible permissive dependencies with notices.

## Next action
Follow [command and UX parity task](../05-command-ux-parity/plan.md): expand the frozen
command contracts and measure native resource budgets, then implement per-pane terminal
surfaces/tabs, sidebar collapse, attention inbox and embedded browser in phased commits.
The new task retains the full original roadmap and PR #1 scope; the delivered development
app is not full command or UX parity.

## Outstanding gates
- This machine is Linux; native builds and automated smoke checks use hosted macOS runners.
- Manual native IME, accessibility, clipboard/selection and responsiveness checks.
- Measured native resource budgets and multi-session/reconnect stress evidence.
- Product/name availability; signing/distribution credentials and cloud provider/budget.
- Full M1/M2 acceptance (including saved layouts/tabs/history) and M3–M7 remain pending.
