# Implementation and delivery evidence

## Behavioral test owners
- Desktop runtime owns applying actions to real PTYs, preserving hidden workspace sessions,
  start/close race cleanup, session limits and input/resize routing. Existing model and PTY
  tests cannot verify their actual integration. No test-only production seam is needed.
- Terminal adapter owns styled grid-run export and Unicode column positions for native drawing.
- Native packaged smoke check owns Swift/C linkage, AppKit view construction, rendered output,
  keyboard input routing, kernel resize and bundle startup. Portable tests cannot prove these.

## Verification
### Rust desktop integration
- `scripts/check`: 22 Rust behavioral tests, C ABI checks, formatting, Clippy and notice inventory pass locally.
- Public desktop tests route real shell input/resize, preserve hidden sessions, reap closed panes,
  reject excess terminals without model mutation and handle close during startup.
- Session launch happens off the action coordinator; closed launch results are canceled/reaped.
- Bound active/starting/retiring processes and queue capacity. No UI-thread process joins.
- Export styled terminal runs with ANSI colors, wide Unicode positions and terminal keyboard modes.
- Swift/AppKit compilation and packaged native smoke evidence are pending the hosted build.

## Remaining gates
Development build is ad-hoc signed, not notarized or release-signed. Interactive IME,
accessibility, selection, performance budgets, remote reconnection, cloud and mobile parity
need their own checks; successful packaging alone is not proof of those behaviors.
