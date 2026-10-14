# Status

- **State:** in-progress
- **Updated:** 2026-10-14

## Notes
- Implementation authorized. [Foundation task](../02-foundation-prototype/plan.md) tracks initial slices.
- Portable foundation complete: MIT setup, Rust workspace/action core, headless PTY/parser, CLI and C ABI feasibility bridge.
- `scripts/check` passes 17 Rust behavioral tests, C ABI checks, formatting, Clippy and dependency inventory.
- Existing-module Linux/macOS CI configured locally; no push or hosted run. M0 remains in progress.
- `origin` is configured as `https://github.com/ashutoshpw/zmux.git`; no push is performed.
- Chosen architecture: Rust shared core/CLI/session services with thin Swift/AppKit macOS UI.
- egui/eframe is deferred. cmux remote-session hangs are unverified motivation for stress gates.
- PR #1 behavior remains required independently authored scope; no service code has landed yet.
- Working defaults: Ashutosh Kumar and Zmux contributors for copyright, macOS 14+ target,
  Rust 1.95, Linux-first remote host support, compatible permissive dependencies with notices.

## Next action
Validate engine rendering and Swift linkage on a macOS machine, then record responsiveness
and resource baselines and establish numerical budgets before marking M0 complete or
expanding the native UI. The current C ABI returns model effects without executing them;
connect the session runtime through the Rust coordinator in the native implementation phase.

## Outstanding gates
- This machine is Linux without Swift, Xcode or Zig; macOS build/dogfood access is needed.
- Native terminal engine/renderer choice and Rust/Swift threading/lifecycle integration.
- Measured native resource budgets and multi-session/reconnect stress evidence.
- Product/name availability; signing/distribution credentials and cloud provider/budget.
- All M1–M7 native/remote/cloud/mobile acceptance criteria remain pending.
