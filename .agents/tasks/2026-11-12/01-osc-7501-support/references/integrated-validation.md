# Integrated validation receipt

**Date:** 2026-11-12
**Checkout:** shared `main`, HEAD `d573e8b87258d426ed8ccdfea20165a6a5fc2b5a`
**Scope:** validation only. Existing dirty work and the real index were preserved.

## Full check

`RUST_TEST_THREADS=2 scripts/check` ran from 2026-11-12T17:05:11Z to 2026-11-12T17:07:54Z and exited **101** (163.0 seconds). `cargo fmt --all -- --check` passed. The workspace test phase stopped at `apps/cli/tests/ssh.rs:166:13` with this partial coverage:

- **121 passed, 3 failed, 3 ignored** across 17 completed test-result lines.
- Failed cases: `managed_ssh_opencode_tracks_root_events_refreshes_long_turns_and_preserves_config`, `managed_ssh_codex_uses_root_thread_events_and_ignores_stop_hook_decisions`, `ssh_cli_opens_an_actual_remote_session_and_reopen_never_replays_its_command`.
- The PTY output showed `fatal runtime error: assertion failed: output.write(&bytes).is_ok(), aborting`; remote lifecycle startup also reported `hello=false, active=false`.

The parent’s read-only probe at `/tmp/zmux-osc7501-arg-probe.rs` measured the exact generated `remote_command()` string at **146,116 bytes**. On this Linux host, page size is 4,096 bytes and `MAX_ARG_STRLEN` is 131,072 bytes. With the terminating NUL, the argument is 146,117 bytes, **15,045 bytes over the limit**. Passing that exact string as one `/bin/true` argument returned `E2BIG` (`os_error=7`, “Argument list too long”). The command content and nonce were not copied into this record. This confirms a current-source Linux argument-size failure consistent with the three SSH cases stopping before hello. The historical core-only green check does not cover this integrated source.

## Supplemental gates

- VTE raw OSC dispatch: `3 passed, 0 failed, 0 ignored, 31 filtered out`; exit 0.
- Non-linked Ghostty compile: `cargo check --locked --offline -p zmux-ghostty-vt` passed; exit 0.

## Gates not completed

- Strict workspace Clippy, license inventory and native bridge checks were not reached because the workspace test command failed.
- Swift/AppKit compilation and packaged sidebar acceptance were not run; the host is Linux x86_64.

## Source fingerprints and limits

The before and after manifests cover **35 changed or untracked Rust/native source files**. Aggregate SHA-256: `8f10212bdc674d7e25bd1e6131a8f4628bbabf72d700b00fade0ea9523c23c39` before and `8f10212bdc674d7e25bd1e6131a8f4628bbabf72d700b00fade0ea9523c23c39` after. They match: **True**. The manifests are `/tmp/zmux-osc7501-integrated-source-before.json` and `/tmp/zmux-osc7501-integrated-source-after.json` and are embedded in the structured receipt. Shell `.sh` and `.py` payloads are outside this Rust/native fingerprint set. The after snapshot describes the pre-repair validation source only; it does not fingerprint later bootstrap-worker edits or the repaired rerun candidate.

## Evidence files and preservation

- Full and supplemental output: `/tmp/zmux-osc7501-integrated-check.log`
- Structured receipt: `/tmp/zmux-osc7501-integrated-check.json`
- A Cargo-generated `vendor/vte/Cargo.lock` was removed after the VTE check because it was absent from the initial checkout status. No files were staged or committed. This reference is the only repository file added by this validation pass.
