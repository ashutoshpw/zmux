# Managed SSH Codex lifecycle readiness fixture repair

**Date:** 2026-11-13

**Related diagnosis:** `managed-ssh-codex-admission-diagnosis.md`

## Change

The managed SSH Codex test now waits, after PTY `running` and before starting
the fixture app-server or Codex command, for the real
`$ZMUX_SHELL_INTEGRATION_DIRECTORY/agent-lifecycle-generation` marker. Its
test-only Python probe follows the production `root_context()` and
`generation_value()` checks in `crates/session-runtime/src/shell/provider-launch.py`:
nonce-scoped `/tmp/zmux-shell-<nonce>` root, owner and mode checks, no-follow
open, regular file, owner, `0600`, single link, 64-byte bound, ASCII UUID, and
non-nil generation. It waits at most eight seconds for a missing marker, with
the production 50 ms poll interval, within the existing ten-second screen
wait. Invalid or missing readiness reports a distinct failure marker before
the test sends any daemon or provider command.

The managed stream's activation path validates `Activate`, publishes the
generation through `publish_generation()`, then sends `Active` in
`crates/session-runtime/src/shell/claude-lifecycle-stream.py`.

The success marker is assembled from separate source fragments and the test
requires it as an exact terminal output line, so the echoed probe command
cannot satisfy the barrier. Before and after the probe, the test asserts that
neither the Codex call-count file nor app-server PID file exists.

The synthetic Codex fixture now prints `CODEX_PROVIDER_PROCESS_ENTERED` only
from its actual `provider()` invocation branch, before SessionStart handling.
It prints `CODEX_PROVIDER_TRACKING_UNAVAILABLE` if that hook fails. The test
requires the existing exact `CODEX_FIXTURE_READY` line for successful tracked
startup; process entry and the untracked fallback do not satisfy it. Metadata,
help, version, and app-server daemon branches do not print the provider-entry
marker. Argument reports, call counts, completion, Stop-hook, identity,
generation, and fallback assertions are preserved.

## Validation

- Focused real loopback SSH Codex test passed:
  `cargo test --manifest-path apps/cli/Cargo.toml --test ssh managed_ssh_codex_uses_root_thread_events_and_ignores_stop_hook_decisions -- --exact --nocapture`
- Full SSH target passed, 3 tests:
  `cargo test --manifest-path apps/cli/Cargo.toml --test ssh -- --nocapture`
- Synthetic Codex dependent passed:
  `cargo test --manifest-path apps/cli/Cargo.toml --test provider_hooks automatic_local_codex_uses_its_exact_real_desktop_terminal_route -- --exact --nocapture`
- Formatting passed: `cargo fmt --manifest-path apps/cli/Cargo.toml --check`.
- Strict scoped Clippy passed:
  `cargo clippy --manifest-path apps/cli/Cargo.toml --test ssh -- -D warnings`.
- Python AST syntax parse and scoped `git diff --check` passed.

## Proof limits

No activation-withholding negative control was added: the existing SSH fixture
has no activation hold control, and mutating or fabricating the
production-owned generation marker would cross the accepted test boundary. The
local test proves the probe observes a validated published marker before the
fixture daemon and provider launch; it does not execute the probe against a
forced missing marker. The hosted route-admission cause remains unknown, and
the recorded `EPERM` cancellation messages are not attributed to that failure.
No production runtime, route budget, timeout, task plan, status, or index was
changed.
