# Status

- **State:** in-progress
- **Updated:** 2026-10-21

## Notes
- Implementation and independent production review are complete; no confirmed correctness or security findings remain. The native tab icon uses the runtime eligibility flag, captured namespace/surface/session identity, and acknowledged dispatch. Packaged smoke clicks the actual icon, checks the replacement terminal view and fresh-shell input, and retains a stopped-icon screenshot.
- Native fixture behavior was exercised with private loopback SSH on Linux. Swift/AppKit compilation and packaged native execution require the hosted macOS workflow and remain pending. The existing save-dialog/accessibility exclusions are unchanged.
- Design accepted: same surface ID, expected old session ID, fresh replacement session ID; reconnect accepts finished non-running SSH processes and settled SSH launch failures.
- Native file drops must carry their originating session ID. Existing upload completion logic already cancels stale `(surface_id, session_id)` origins.
- Preserve existing dirty cloud-origin task status and untracked profile-lifecycle files.
- Rust runtime and workspace action are implemented. Reconnect is admitted from `Worker::action` only for an exited SSH process or a settled launch failure, pins the expected surface/session, and clears the startup command on replacement. An exited process releases process capacity before its history worker is joined; the retired wrapper remains queued for asynchronous cleanup.
- Updated `docs/ssh.md` and the SSH CLI help description to distinguish the manual same-tab fresh-login icon from shell resume, automatic/durable reconnect, and CLI reconnect.
- Focused verification passed: `cargo test -p zmux-desktop-runtime --lib ssh_reconnect -- --nocapture` — 3 passed, 0 failed.
- The first full `RUST_TEST_THREADS=2 scripts/check` run exited 101 because `crates/native-bridge/tests/swift_file_contract.rs::pin_a_close_the_core_refuses_gives_the_discard_back` still omitted `.reconnectSSH` from its tracked-command allowlist. Updated the contract and added a check that `NativeBridge.reconnectSSH` sends an acknowledged action. Focused rerun passed: 1 passed, 0 failed.
- Final full verification passed: `RUST_TEST_THREADS=2 scripts/check` exited 0. It ran all workspace tests, doc tests, Rust checks, package inventory, C ABI checks, and native-profile-lease C ABI checks. The reconnect runtime tests all passed in that run.
- Packaged native smoke and hosted macOS acceptance remain pending; task delivery stays in progress.
