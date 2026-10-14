# Status

- **State:** in-progress
- **Updated:** 2026-10-14

## Notes
- Implementation authorized. [Foundation task](../02-foundation-prototype/plan.md) tracks initial slices.
- First slice adds MIT project setup and the original Rust workspace/action core; portable checks pass.
- `origin` is configured as `https://github.com/ashutoshpw/zmux.git`; no push is performed.
- Chosen architecture: Rust shared core/CLI/session services with thin Swift/AppKit macOS UI.
- egui/eframe is deferred. cmux remote-session hangs are unverified motivation for stress gates.
- PR #1 behavior remains required independently authored scope; no service code has landed yet.
- Working defaults: Ashutosh Kumar and Zmux contributors for copyright, macOS 14+ target,
  Rust 1.95, Linux-first remote host support, compatible permissive dependencies with notices.

## Next action
Complete the portable PTY/parser and native-bridge feasibility slices. Then validate engine
rendering and Swift linkage on macOS, record responsiveness/resource baselines, and establish
numerical budgets before marking M0 complete or expanding the native UI.

## Outstanding gates
- This machine is Linux without Swift, Xcode or Zig; macOS build/dogfood access is needed.
- Native terminal engine/renderer choice and Rust/Swift threading/lifecycle integration.
- Measured native resource budgets and multi-session/reconnect stress evidence.
- Product/name availability; signing/distribution credentials and cloud provider/budget.
- All M1–M7 native/remote/cloud/mobile acceptance criteria remain pending.
