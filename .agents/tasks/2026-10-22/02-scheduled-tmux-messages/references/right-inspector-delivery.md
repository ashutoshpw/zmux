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

## First native pass

Source `966b68fa41115516b2ff51fd71594d332913cf9e` is pushed on main. Run
`39928042752` passed the fast actual-AppKit gate (ten queue cases and all 16
inspector combinations, draft retention and setup popover), both architecture
source checks and both Linux helper builds. Root downloaded fast evidence
artifact `12489459839` to `/tmp/zmux-right-inspector-layout-39928042752/`.
Personal post-commit source review found no new scheduler state/route defects.

Image inspection exposed excess space in the inspector title row and restoration
to a wider pane after hide/show. Pin that row to its native 18-point height,
give surplus vertical space to the message editor, preserve the previous width,
and explicitly resize siblings within the fixed window on collapse. Extend the
native component check for top alignment and width retention. Paint the native
background in the production owner so component captures have the same opaque
appearance as the host; capture the dark appearance used by the application.
Full local recheck passed with exit 0 at
`/tmp/zmux-right-inspector-alignment-check.log`; native follow-up is pending.

Source `86602d8` passed the expanded native gate including all 16 top-aligned
cases, width/draft retention and setup popover, plus both Swift architectures.
Evidence: `/tmp/zmux-right-inspector-layout-39929007328/`. Its dark native capture
showed that the remaining flexible time-entry stack could still absorb spare
height. Bound that row to a native control height, give fixed labels/pickers
required vertical hugging and leave expansion to the message editor. Add a
geometry assertion for the gaps around the time controls. This closes the gap
in the first layout assertions instead of weakening the visible-content checks.
The repeated full local gate passed with exit 0 at
`/tmp/zmux-right-inspector-final-check.log`. The final native visual gate remains
pending; do not treat an earlier candidate as the accepted right-inspector build.

Final layout source `cbc7ac3ced002ff8a519d17c6b7c517c82396f21` is on main.
Run `39929941810` passed all ten queue cases and all 16 full inspector cases,
including compact time-control gaps, top alignment, width/draft retention and the
setup popover. Root downloaded the actual native report and dark screenshot to
`/tmp/zmux-right-inspector-layout-39929941810/` and visually confirmed the compact
form with spare space going to the message editor. Full app compilation and
packaged scheduler acceptance remain pending. Personal post-commit review of the
follow-up changes found no new state/transport changes or actionable defects.

## Accepted build

Run [39929941810](https://github.com/ashutoshpw/zmux/actions/runs/39929941810)
completed successfully for source `cbc7ac3ced002ff8a519d17c6b7c517c82396f21`.
Both Swift architectures, both Linux helper builds, the full Mac Rust/C ABI gate,
universal build, terminal-selection/browser-link journey, packaged rendering and
scheduler suite, sidebar and system-status-menu checks passed. All **16 packaged
scheduler checks** passed, including the setup popover and dirty-draft inspector
collapse/reveal, alongside the existing create/edit/cancel/relative-time/receipt
and CAS contracts. The fast actual-AppKit gate passed ten queue cases and 16 full
inspector cases plus draft/width retention and setup-popover checks.

Final download:
[artifact 12491858416](https://github.com/ashutoshpw/zmux/actions/runs/39929941810/artifacts/12491858416),
`zmux-dev-universal-cbc7ac3.zip`, 24,394,882 bytes, SHA-256
`2c8b62dbdc5cf20cb3789d1983fbfd4171c68a46976750b8d34d340a13d17383`.
Root downloaded it to `/tmp/zmux-right-inspector-delivery-39929941810/` and
extracted the application to `/tmp/zmux-right-inspector-app-39929941810/`.
`download-verification.json` records matching ZIP/checksum, outer/bundled build
metadata and Info source, macOS 14 minimum, four universal Mach-O binaries, all
three scheduler helper manifests/hashes/modes, 16 scheduler checks, five distinct
Applied mutation receipts, original-version CAS rejection, and completed
same-source terminal-selection acceptance.

Root inspected the actual packaged `zmux-scheduled-messages.png`: the embedded
scheduler has a compact toolbar, queue and native right inspector, readable message
content, compact time controls, and actions/status within the window. No extra
scheduler window is introduced. The inspector is resizable and its collapse test
preserves the dirty message, selection and enabled Save action.

Build is universal Apple Silicon/Intel, requires macOS 14+, is ad-hoc signed and
not notarized. Hosted fixture checks do not prove behavior on the user's physical
Mac or production remote account. The established accessibility Save-test deferral
is unchanged. This delivery does not install a production scheduler or create any
production scheduled messages. Final receipt commit changes documentation only;
the downloadable application's source remains `cbc7ac3`.
