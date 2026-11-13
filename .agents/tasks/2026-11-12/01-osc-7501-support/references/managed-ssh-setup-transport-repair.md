# Managed SSH setup transport fixture repair

**Date:** 2026-11-13
**Source under repair:** `2efb76d9bcbb9e0a00015602b8bcaf81c00920de`
**Scope:** `apps/cli/tests/ssh.rs` and this reference only.

## Baseline evidence

Foundation run `43407437043` passed at the source SHA. Universal run
`43407437131`, job `131826686970`, failed in the SSH integration target. The
retained full job log is
`/tmp/zmux-osc7501-final-2efb76d/universal-failed-job-131826686970-full.log`.
The Codex terminal capture showed garbled Python received over the PTY and a
`SyntaxError` at line 42, including the received fragment
`value = os.read(desc                ve("a   i").strip()`. The checked-in
`CODEX_READINESS_PROBE` source parsed as valid Python; the failure showed that
the multiline here-doc transport was not reliable in this fixture, not that
the source parser was broken.

The same job showed OpenCode's one fallback after provider-route admission was
not confirmed and `hello=false, active=false`. This does not establish why
admission failed. The hosted no-Hello/admission cause remains unproven. The
retained Ghostty linkage run `43407437492` resolved all 229 source pins,
compiled and linked both Apple targets, and passed its ARM64 suites. This repair
does not alter or repeat that work.

## Repair

The loopback SSH fixture shares its filesystem with the test process and uses a
unique mode-0700 root. The test now writes the readiness Python source and each
provider's environment setup to new files under that root. Creation uses
`create_new`, mode `0600`, and checks regular-file type, matching root owner,
single link, exact mode, and complete bytes before the file is used. This
prevents overwriting a preexisting file and makes the entire payload available
before the remote shell starts it.

The readiness command is now the short, quoted
`python3 '<fixture path>'` command. Both tracked Codex and tracked OpenCode run
the same nonce-scoped generation barrier after PTY `running` and before their
fixture provider is launched. The probe keeps the eight-second generation wait
and the production-aligned nonce, root, ownership, permission, no-follow,
regular-file, link-count, size, ASCII UUID, and nonzero-generation checks. Its
fragment-built `ZMUX_MANAGED_SSH_LIFECYCLE_READY` or `..._FAILED` marker is
required as an exact terminal line. The short PTY command is asserted not to
contain the READY marker, so command echo cannot satisfy the barrier. The
existing ten-second screen deadline remains in use.

Codex and OpenCode environment exports are also stored in private setup files.
The real remote shell sources each file with a short quoted path command, then
runs the original provider command. Existing PATH behavior, environment
values, provider arguments, OpenCode config and plugin contents, and Codex
daemon arguments are retained. The readiness precondition verifies no provider
call/report or Codex daemon PID exists before and after the barrier. Provider
tracking, exact READY/once checks, config preservation, root identity,
generation, heartbeat, completion, Stop-hook, and fallback assertions remain
in place. The general PTY input fixture and production code are unchanged.

## Validation

- `cargo test --manifest-path apps/cli/Cargo.toml --test ssh --no-run` compiled
  the SSH integration target.
- `cargo test --manifest-path apps/cli/Cargo.toml --test ssh -- --nocapture`:
  **3 passed, 0 failed**. This includes both real loopback managed-provider
  tests and the ordinary SSH session/reopen test.
- `cargo test --manifest-path apps/cli/Cargo.toml --test provider_hooks automatic_local_codex_uses_its_exact_real_desktop_terminal_route -- --exact --nocapture`:
  **1 passed**.
- `cargo test --manifest-path apps/cli/Cargo.toml --test provider_hooks automatic_local_opencode_tracks_exact_root_and_preserves_user_configuration -- --exact --nocapture`:
  **1 passed**.
- `cargo fmt --manifest-path apps/cli/Cargo.toml --check` passed.
- `cargo clippy --manifest-path apps/cli/Cargo.toml --test ssh -- -D warnings` passed.
- `git diff --check -- apps/cli/tests/ssh.rs` passed.
- Each SSH test parsed the exact prepared readiness file bytes with Python
  `ast.parse`, checked each exact setup file with `/bin/sh -n`, and verified
  fixture file ownership, permissions, link count, and complete contents before
  remote use. The shell scripts were sourced through the real loopback SSH
  PTY, and the exact readiness markers were observed.

## Proof limits and handoff

These local real-PTY tests validate the fixture transport and readiness
ordering. They do not rerun the historical hosted job or prove the cause of the
OpenCode no-Hello/admission failure. `scripts/check`, hosted workflows, app
builds, and native acceptance were not run as part of this bounded repair.
The passing local SSH run also printed lifecycle startup-retry diagnostics
(`hello=false, active=false`) before completing; they are retained as local
observations and are not attributed to the historical hosted failure.
No plan, status, production source, workflow, app, index, commit, or remote
state was changed.

## Parent review follow-up

Parent fixed the review P2: provider invocation is chained to successful setup
sourcing with `&&` for both Codex and OpenCode. A source failure cannot proceed
to an executable from the ambient PATH. Runtime validation of the final chains
is deferred until the concurrent Desktop diagnostic source writer finishes.
