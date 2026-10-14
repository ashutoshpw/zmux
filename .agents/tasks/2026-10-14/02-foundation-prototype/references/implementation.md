# Implementation evidence

## Test authoring decisions
No pre-existing code or tests exist. Tests exercise production APIs rather than source text.
- Workspace action tests protect stable identity, focus validity, split collapse, close effects,
  capacity limits and unchanged state after invalid actions. Credible failures include dangling
  focus after closure, stopping the wrong session and partial mutation on rejected commands.
  The public Rust action boundary is the primary owner; no test-only production seams are needed.
- PTY tests will own actual process I/O, parser ordering, resize and cleanup; core tests cannot
  reach OS process lifecycle or asynchronous cancellation.
- C ABI tests will own independent calling convention, memory/handle ownership and decoding;
  Rust unit tests cannot establish the foreign caller contract.

## Verification
### Slice 1 — MIT bootstrap and workspace actions
- `scripts/check`: passed formatting, six public-action tests, and Clippy with warnings denied.
- Original core performs no PTY/network/UI I/O; successful transitions return session effects.
- Limits: 32 workspaces, 64 panes/workspace, split depth 16 and 128-character names.
- MIT notice uses Ashutosh Kumar and Zmux contributors, matching repository authorship.
- Rust runtime graph reviewed from resolved Cargo manifests; all current declared licenses
  are permissive, including Unicode-3.0 for unicode-ident (see `THIRD_PARTY_NOTICES.md`).
- No native UI, session runtime, deployment, or PR #1 service functionality exists yet.

### Slice 2 — bounded headless PTY feasibility
- `scripts/check`: 16 behavioral tests pass (6 workspace, 6 real PTY, 4 parser), plus formatting and Clippy.
- `cargo run --locked --quiet -p zmux -- prototype -- /bin/sh -c 'printf "Zmux PTY ready\n"'`:
  exited successfully; parsed visible screen is `Zmux PTY ready`, exit code 0.
- Actual PTY checks cover output/exit status, stdin and kernel resize, output-flood cancellation,
  queue saturation, invalid requests, nonblocking client drop and leader reaping.
- Original control-string limit regression was demonstrated red and green with the same command:
  `cargo test -p zmux-terminal-adapter unterminated_control_strings_fail_explicitly_instead_of_growing_without_bound`.
  Before the guard, the assertion failed because the parser accepted an oversized OSC string;
  after the guard, it reports the explicit limit error. Valid Unicode and terminated controls pass.
- vt100/filedescriptor/portable-pty API and ownership were reviewed directly from registry sources.
- CLI execution timeout was checked with a sleeping command: reports an execution failure and returns after cleanup.
- Limits are conservative prototype guardrails, not measured native performance budgets.

## Native gates
No native UI or signed artifact can be validated on this Linux machine. A macOS machine
must establish Swift linkage, rendering/IME/accessibility, input latency, CPU/memory budgets,
sleep/wake behavior and real remote-session stress evidence before M0/M1 are marked complete.
