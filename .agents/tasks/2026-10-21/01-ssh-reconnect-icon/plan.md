# SSH reconnect icon

**Date:** 2026-10-21
**Task:** 01-ssh-reconnect-icon

## Objective
Let a managed SSH terminal reconnect from its existing tab after the process exits or its launch fails. Keep the tab and route, start a fresh session, and never replay the saved startup command or prior input.

## Context
SSH route metadata is stored on `Surface`; `for_restore()` preserves routing while clearing the explicit command. Terminal tabs are rendered in AppKit and session launches are owned by the Rust desktop worker. Reconnect does not add durable sessions, menus, tmux behavior, or a CLI command.

## Approach
Add a Rust action pinned to both `SurfaceId` and expected `SessionId`. Admit only an ended/failed managed SSH session or a completed launch failure; retain the surface and route, allocate a new session ID, and validate replacement capacity against the stopped session. Show one reconnect icon on eligible SSH tabs. Pin native file drops to the view's session ID so stale callbacks cannot target the replacement.

## Steps
- [x] Implement workspace action, runtime eligibility/capacity checks, and stale-operation containment.
- [x] Add the tab icon and native packaged smoke using the existing loopback SSH fixture before the clipboard stage.
- [x] Update `docs/ssh.md` and run focused checks plus `RUST_TEST_THREADS=2 scripts/check`.

## Acceptance criteria
- [x] Same surface, pane, order, title, and saved route; replacement gets a fresh session ID.
- [x] Running, pending, local, and stale-session requests are rejected; finished SSH exits and failed launches can retry.
- [x] Reconnect clears the saved explicit command and prior input is never replayed.
- [x] A replacement works at the 32-terminal limit when the ended target is the only capacity reclaimed; retirement stays asynchronous.
- [x] Native smoke clicks the real icon and verifies its visibility and disappearance across lifecycle states.

## Test authoring answers
1. **Contract:** Only a managed SSH session whose process has ended or settled SSH launch failure can be replaced, using its exact surface/session identity; the surface remains and the new route has no command.
2. **Credible regressions:** A stale/double click replaces a newer session, a local or live/pending session is replaced, retry fails at the terminal cap, a failed SSH launch cannot retry, an old file-drop callback uploads through the new session, or command/input is replayed.
3. **Coverage gap:** Existing reopen coverage proves closed-tab restore behavior, but there is no in-place reconnect action, capacity replacement proof, or clickable native recovery affordance. Rust owner-boundary tests cover lifecycle and capacity; packaged AppKit smoke covers actual icon dispatch and visibility.
4. **Production seam:** None. Use the workspace action, real desktop/session owner, existing SSH fixture, existing native bridge, and actual AppKit control.

## References
- [status.md](status.md) — implementation and verification state.
- [references/artifact-verification.json](references/artifact-verification.json) — verified universal artifact, native receipts, screenshot and source hashes.
- [../../2026-10-14/01-build-zmux/plan.md](../../2026-10-14/01-build-zmux/plan.md) — project architecture and session reliability decisions.
