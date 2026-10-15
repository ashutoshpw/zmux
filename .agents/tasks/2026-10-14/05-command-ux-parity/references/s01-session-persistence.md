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

## Native physical storage path repair (2026-10-15)

Run `38809101107` at `11671a48ddb484f8730ddf4df43e6a5394d7e886`
passed Rust/core/ABI checks, both native architectures and the close-phase journey.
The final private session document was saved, and shared shutdown joined every
physical owner. Reopen published revision6/model0 with no windows. Its public
session/profile diagnostics both report `open private persistence state directory:
Not a directory (os error 20)`; session/profile loading had ended, and saved session
and generation documents were unchanged. The diagnostic artifact is `12008352484`,
ZIP SHA256 `9a18d5a751104ba0159634fd428e69e3dcc4cdc1f83708de16751cc912bdaa2a`.

The native bridge passed the profile through Foundation's symlink resolver.
[Apple documents](https://developer.apple.com/documentation/foundation/nsurl/resolvingsymlinksinpath)
that this API strips `/private` when the shorter path exists. The fixture's
physical `/private/tmp/.../profile` is absent on first launch but exists on reopen;
the shortened `/tmp` component is a symlink and the Rust nofollow walk refuses it.
The same failure affects both users of that state directory before restore starts.

The bridge now uses POSIX `realpath` on the nearest existing directory and appends
only validated missing names. It ascends only on ENOENT; permission, non-directory
and symlink-loop errors remain errors. Existing regular-file ancestors are refused.
Relative, dot, dotdot and NUL paths and non-UTF8 resolved target bytes are refused.
Both explicit and Application Support
storage use this owner, off AppKit's main actor. The physical string is passed
directly to Rust without subsequent Foundation normalization. Rust's component
nofollow walk, private directory checks, reservations and restore logic are unchanged.
Canonicalization does not authorize a later symlink replacement: Rust still reopens
each component with its strict flags.

The existing packaged close/save/reopen journey is the pre-fix behavioral red.
An early focused native check also exercises the production path owner with real
missing/existing directories under `/private/tmp`, the OS `/tmp` alias, strict
nofollow opens, and invalid/file/loop/raw-byte refusals. The raw-byte case includes
a different existing replacement-character directory, so lossy decoding cannot
silently select it. Its Foundation control must reproduce
the old existing-path ENOTDIR. It runs before Rust compilation so path regressions
fail early. Shell syntax, workflow YAML/order and whitespace checks pass on Linux;
no local Swift compiler or Darwin SDK is available. Focused native execution and a
green packaged reopen remain hosted gates, including split ratio and fresh input.

The first repaired run `38811810709` passed strict Swift typechecking on both
Apple targets, then passed the focused main path/alias and invalid/file/loop
assertions before failing to create the optional raw-UTF8-negative target. A
diagnostic-only rerun `38812458734` at `a2e3d09bfbee326076dffe926ff472b5244d0aa3`
captured the actual `mkdir` result: `-1`, errno92. The public XNU `bsd/sys/errno.h`
defines92 as EILSEQ, illegal byte sequence. The fixture filesystem refuses that
invalid filename before the resolver can observe it; no filesystem-format cause
is inferred. That one case now records only this specific refusal as its platform
outcome. Other creation errors still fail. If a filesystem permits the raw name,
the original production-owner invalid-UTF8 refusal assertion still runs, including
the usable replacement-character directory. All main path assertions and the
production lossless decoding guard are unchanged. Native focused and packaged
reopen acceptance remain pending after this fixture-only correction.
