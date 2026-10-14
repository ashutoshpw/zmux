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
  are MIT OR Apache-2.0 (see the version inventory in `docs/dependencies.md`).
- No native UI, session runtime, deployment, or PR #1 service functionality exists yet.

## Native gates
No native UI or signed artifact can be validated on this Linux machine. A macOS machine
must establish Swift linkage, rendering/IME/accessibility, input latency, CPU/memory budgets,
sleep/wake behavior and real remote-session stress evidence before M0/M1 are marked complete.
