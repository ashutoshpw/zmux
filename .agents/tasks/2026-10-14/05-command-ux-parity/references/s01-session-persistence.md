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
Native Actions at 5f26fbe passed both Foundation checks (38728287986) and universal
build/packaged smoke (38728288079). Earlier CI exposed fixture /tmp canonicalization,
allocator-test backpressure assumptions and Darwin mkdtemp declarations; all were
repaired without weakening production path or allocation checks.

## Remaining S01 gates

This is not full session parity. Explicit import requires an empty desktop; additive
window import, channels, multiwindow, native divider ratios, undo/navigation histories,
full browser history/state and running-job resurrection remain open. Safe preflight quit cancellation and native failure-alert interaction remain open. The full V01 artifact gate stays open.

## Concurrent lock-open repair (2026-10-15)

macOS run `38771925429` at `93daf02` failed the unchanged concurrent restoration
contract before native compilation: `open persistence generation lock` returned
ENOENT, although diagnostics subsequently observed both the lock and ledger in the
private UUID fixture. No lock-unlink path or early fixture teardown was found.
The failure is observed; an APFS/kernel root cause is not established.

A dedicated dispatch-only workflow compiles that existing test once and runs it
against 128 fresh fixtures, retaining the first failure without weakening expected
outcomes. Pre-fix run `38773075613` at `dd7a090` completed 128 successful attempts
(`no_failure_observed`), downloaded to
`/tmp/zmux-persistence-concurrency-prefixed-38773075613`. This nonreproduction does
not invalidate the earlier hosted failure. The initial workflow configuration was
rejected for using runner context in job environment; `dd7a090` resolves the target
directory inside the executing step.

The repair retries only ENOENT, at most three times, on the same anchored directory
and unchanged no-follow/nonblocking/close-on-exec open flags. Descriptor ownership
validation and exclusive lock admission still precede any ledger read/write.
Persistent disappearance and other failures retain their explicit errors. Existing
and dangling lock symlinks are refused without creating/changing targets or a ledger.
All 14 focused persistence tests and strict Clippy pass locally. Parent independent
`scripts/check` passes in `/tmp/zmux-lock-retry-parent-verify-ef2fsvhd` with a fresh
private Cargo target (log `/tmp/zmux-lock-retry-parent-check.log`). Source review
accepts the bounded retry without weakening lock ownership. Post-fix diagnostic run `38773735612` at `610e897` also passed all 128
attempts; its downloaded receipt is in
`/tmp/zmux-persistence-concurrency-postfix-38773735612`. Both pre-fix and
post-fix diagnostics passed, so these receipts do not establish the OS root cause.
Native build validation remains pending.
