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

## Reassessment after the third native attempt

The original repair did not resolve the native column extent. Run 39875315431
reports a 483-point table/viewport with a State column ending at 523 points.
Stop repeated broad builds and isolate the real queue geometry owner first.
`ScheduleQueueView.swift` is the application's actual scroll/table configuration;
the native probe compiles that file directly without a bridge stub or test-only
production hook. Its invariant is that every column stays within the visible
viewport through initial 483-point, narrow, wide and repeated resize transitions.
Existing packaged acceptance remains necessary for the parent pane, editor and
SSH lifecycle; the fast component check provides distinct resize feedback before
any expensive build. First demonstrate the existing failure, then repair sizing.

Local `scripts/check` passed at `/tmp/zmux-relative-tabs-queue-owner-final-check.log`.
The first invocation hit an unrelated existing terminal/browser-link `Busy` error;
an unchanged focused rerun passed, followed by the successful full gate. No Rust
or link test code changed. Python compilation and workflow YAML parsing passed.

## Native red result and explicit spacing fix

Fast run 39879610961 reproduced the same failing 483-point case using the actual
production owner. Its native JSON reports unchanged column widths [150,100,110,90]
and automatic-style intercell spacing (17,0): State extends to 523 although the
viewport/table width is 483. Evidence is at `/tmp/zmux-queue-layout-red-39879610961/`.
Use explicit `fullWidth` style and zero extra intercell spacing, preserving the
cell renderer's own padding. Extend the same owner-level resize cases to overlay
and legacy scrollers, including enough legacy rows to require a vertical scroller.
The original packaged geometry assertion is unchanged. The full portable gate
passed at `/tmp/zmux-relative-tabs-spacing-check.log`. Native green is still required.


## Final accepted result

Source `93f36fcab3ac89fa0fa32639a8b140f5bfd5066a` passed native run 39880328272.
The component-owner regression is red on 21730fc and green on 93f36fc (10 native
resize cases). The packaged 14-check scheduler journey, including strict geometry,
relative create/edit/countdowns and exact message-only deadline preservation, passed.
Archive source/checksum/universal binaries/helpers and native reports were verified;
the final screenshot shows the full State/version column and readable editor.

No additional actionable defects were found in the final personal read-only review.
The native defects discovered after earlier reviews are fixed and verified; earlier
unrelated scheduler follow-ups remain tracked. Physical-user-Mac and production
remote-account acceptance remain outside the hosted-fixture proof.
