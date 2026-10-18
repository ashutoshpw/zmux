# Status

- **State:** done
- **Updated:** 2026-10-18

## Notes
- 2026-10-18: raised the desktop terminal limit from 8 to 32 at the user's request. The
  shared limit also governs restoration, multiplexer admission and CLI capabilities;
  pending launches and retiring processes remain counted. `RUST_TEST_THREADS=2 scripts/check`
  passed on Linux, including real 32-session admission, overflow rejection and cleanup.
  Capacity fixtures now avoid unrelated split-depth/workspace limits and reject a saved
  session with 33 terminals. Native macOS build/dogfood at the higher limit remains pending.
- Delivered universal macOS development artifact for source `ee2599abc05b51f8459b7a0395c2bfa9e78c336f`.
- [Native workflow run 38630706232](https://github.com/ashutoshpw/zmux/actions/runs/38630706232) passed Rust/C checks, universal Swift/Rust compilation, ad-hoc signing and packaged native smoke.
- [Download artifact 11926235790](https://github.com/ashutoshpw/zmux/actions/runs/38630706232/artifacts/11926235790), retained until 2026-10-28. Manual workflow dispatch creates fresh builds.
- Downloaded and checked ZIP integrity, checksum, executable permissions, arm64/x86_64 binary headers, metadata and bundled license/notices against the repository.
- Native smoke passed shell input, ANSI color, split panes, kernel resize, workspace switch, pane closure and native viewport rendering.
- WindowServer capture unavailable on the runner (status 1); cached viewport rendering is present, but composited control labels require manual native dogfood.
- This task completes development artifact delivery, not M0–M7 or full cmux parity. Remote/cloud/PR #1 behavior, persisted layouts/tabs/history, resource budgets, Intel runtime, IME and accessibility checks remain open.
