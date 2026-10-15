# S01 session persistence foundation

## Implemented contracts

- Separate versioned topology descriptors preserve workspace/pane/surface identities,
  selected tabs, focus, terminal cwd and command-free SSH connection descriptors.
  No terminal input/output, credentials, startup commands or runtime session handles.
  Browser query/fragment and observed page titles are omitted.
- Filesystem worker owns bounded load/save queues, private files, anchored no-follow
  traversal, atomic no-clobber export/replacement, and durable identifier reservations.
  4096-ID blocks cover ordinary allocations as well as restored terminals; crash/reimport
  burns unused IDs. Empty/browser-only imports adopt the reserved floor too.
- Runtime validates and launches a candidate before replacing the authoritative model.
  Failed/late launches are cleaned up; revisions advance monotonically. Corrupt startup
  data remains intact and disables automatic overwrites with a visible error.
- Autosave coalesces committed topology; shutdown attempts a bounded final flush.
- Native production startup requests persistence in Application Support/app.w3dev.zmux.dev.
  Packaged smoke remains isolated. C ABI validates foreign path bytes and exercises
  actual private-file creation through persistent native constructor and shutdown.
- CLI supports restore-session, --from, --export/--force, save-session and session-status
  --id. Admission is asynchronous; completion receipts are authoritative. Read-only
  access can inspect status but cannot import/export/save; remote relay remains denied.

## Verification

Core validation, real-filesystem worker, real-PTY lifecycle and CLI/socket tests cover
separate owner boundaries. Parent scripts/check passed 123 Rust tests, strict
Clippy, formatting, notices and C ABI. Fresh gpt-6.1-sol storage/core review passed.
Integration review found stale OSC7 cwd capture and swallowed final-save failures; both
were repaired and independently reproduced green through real runtime APIs.
Native errors now show a concrete Quit acknowledgment; teardown is already irreversible.
Native Actions compilation/execution remains pending for this slice.

## Remaining S01 gates

This is not full session parity. Explicit import requires an empty desktop; additive
window import, channels, multiwindow, native divider ratios, undo/navigation histories,
full browser history/state and running-job resurrection remain open. Safe preflight quit cancellation and native failure-alert interaction remain open. The full V01 artifact gate stays open.
