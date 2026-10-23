# Native right inspector delivery

The user accepted desktop proposal 1 on 2026-10-23 and requested implementation
and a new downloadable build. Existing task and prior remote scheduler behavior
remain in scope; no unrelated dirty work is included.

## Implementation

- Make the scheduler a bounded AppKit view rather than a 950 x 690 scroll document.
- Use native compact toolbar controls, a resizable queue/right inspector split,
  an unstriped table, pinned editor actions and a one-line status bar.
- Move setup diagnostics/check/install into an accessible native popover while
  keeping urgent errors and unresolved-request actions discoverable.
- Retain the current state owner, source binding, offline drafts, request receipts,
  exact message-only deadlines, history and strict local-time validation.
- Reuse the production layout owner in fast AppKit geometry checks, and retain
  the packaged SSH/helper acceptance journey for behavior and visual evidence.

## Test authoring decisions

1. The observable contract is that the entire queue/inspector/editor actions fit
   inside the selected desktop viewport with readable message glyphs; hiding,
   resizing and restoring the inspector must retain its draft and controls.
2. Credible failures include the current minimum 690-point document overflowing a
   670-point window, long connection/setup text pushing controls offscreen, and
   rebuilding the inspector losing pending local edits.
3. Existing fast tests cover only the queue's columns. Extend them at the actual
   presentation owner for the full bounded view. The existing packaged geometry
   check currently runs in a 1400 x 900 window; extend it at the screenshot's
   size and preserve all remote create/edit/receipt/CAS/countdown checks.
4. No test-only production exports or bridge stubs: the application will use the
   same standalone AppKit presentation view compiled by the fast fixture.

## Validation and delivery

Implementation is complete. Full local `RUST_TEST_THREADS=2 scripts/check` passed
with exit 0 (`/tmp/zmux-right-inspector-check.log`): workspace tests, strict Clippy,
formatting, license inventory and both C ABI executables. Python compilation and
`git diff --check` also passed.

The fast native fixture now compiles the actual queue and full presentation owner:
ten existing queue resize cases plus 16 full-view size/time/scroller combinations,
inspector collapse/draft retention and setup-popover opening. The packaged native
journey now runs at 1034 x 670, retains all 14 scheduler contracts, and adds native
setup-popover and dirty-draft collapse/reveal checks. Native gates remain pending.

Pending: post-commit personal review, hosted macOS build/acceptance, actual archive
download/checksum/metadata verification and updated screenshot inspection. Linux
has no AppKit compiler; portable checks do not establish native UI acceptance.
