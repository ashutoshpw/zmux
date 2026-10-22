# Relative scheduling and embedded Schedules tab review

Reviewed commit: `13ccf1c57da936d91f37839a7f1c35e4c9e44dee`.

Personal read-only post-commit review following `review-agent`: inspected the full
change and its production callers, source/session identity checks, persistence,
no-PTY surface lifecycle, native panel retention and countdown ownership, CLI
parser/output contracts, help and behavioral verification. No delegation.

**No findings.** No new actionable defect was confirmed. Native Swift compilation,
embedded layout/visual acceptance and packaged relative create/edit remain hosted
Mac gates; local Rust tests are not substitutes for them.

`RUST_TEST_THREADS=2 scripts/check` passed with exit 0, including all workspace
behavioral tests, strict Clippy, formatting, license inventory and both C ABI checks.
Log: `/tmp/zmux-relative-tabs-final-check.log`.

The intentional rollback boundary is documented: every saved session uses format
11, which older builds reject, while readers for formats 1–10 remain. Closing an
SSH source leaves a Schedules tab offline; undo-close does not rebind it to a new
surface ID. Only the binding is persisted, not messages or drafts.

Two prior scheduler review issues are addressed by this slice: message-only
editing now preserves the exact deadline without validating a rounded minute,
and New from History returns to the editable pending view. The other previously
recorded scheduler findings were not re-audited or remediated in this slice.

## Hosted follow-up

Hosted run 39867091305 exposed a missed fixture assumption in
`scripts/check-native-windows:16`: it required format 10 before checking the saved
window document, whereas this change writes format 11. The fixture is updated to
require exactly 11; no application code or other assertion changes. All executable
session-version expectations were searched across scripts/tools/apps/crates.
The full local gate passed again at `/tmp/zmux-relative-tabs-fixture-check.log`.
A new hosted run is required; scheduler UI acceptance was not reached in the first.

## Embedded queue layout repair

Run 39870897287 passed save/reopen and the first eight scheduler checks, then
reported the State column outside its visible queue viewport. The retained native
screenshot confirms the embedded tab and real clipping. The layout repair pins
the table frame to its clip width in `NSScrollView.tile`, permits all columns to
autoresize and uses uniform column resizing. The existing strict visibility test
is retained, with actual dimensions added to its failure diagnostic.
Full portable verification passed again at `/tmp/zmux-relative-tabs-layout-check.log`.
Native acceptance remains required; relative create/edit/countdown checks run after
the layout check and have not yet been reached.
