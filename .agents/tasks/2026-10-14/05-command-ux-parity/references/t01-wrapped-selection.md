# T01 wrap-aware copying within the displayed viewport

## Scope and decision

Extend the accepted native selection owner with authoritative soft-wrap joins for
the current visible viewport, including a viewport parked in primary history.
Keep native double/triple-click, character/word/row drags, exact UTF8 and explicit
same-row edge spaces. Triple-click still selects a physical row. Wrapped-word
expansion, rectangular selection, persistent scrollback anchors, drag autoscroll,
keyboard copy mode and mouse protocols are separate slices.

Parent authorized terminal-adapter metadata, native Models/TerminalSelection and
associated production-owner tests/native fixture. Storage reviewer owns no optional
exporter edits and accepted unavailable optional metadata. Own `engine.rs` only for
an explicit unavailable result; do not change ghostty-vt/C wrappers, engine pins,
resource policy, session input/queue owners or sibling work.

## Source findings

The accepted pre-wrap `TerminalSelection.text` trims crossed rows then inserts a
newline at every physical row. Its `Viewport` had runs and aggregate history
offsets, with no row wrap/identity metadata. `TerminalView` clears selection on explicit history wheel,
navigation, offset change and resize. Selecting historical rows already displayed
is source-supported; persistent selection while scrolling is not. Output at a fixed
offset has no stable logical-row anchors and remains a separate risk.

Default vt1000.16.2 exposes public `Screen.row_wrapped`, `Cell.has_contents` and
`Cell.is_wide`. Current drawing runs replace empty cells with a literal space, so
runs alone cannot distinguish written boundary spaces from padding/wide-wrap gaps.
Optional Ghostty RawFrame exports pending cursor wrap, not per-row wrap metadata;
that cursor bit must never be used to infer row boundaries.

Frozen cmux3ba653b9 behavioral contracts preserve spaces at soft-wrap boundaries,
retain hard newlines/blank lines and keep copy-mode scrolling local without
modifying application DEC modes. This design uses independently authored MIT
metadata/copy logic and tests; no cmux code/tests/prose are imported or translated.

## Bounded metadata contract

Add optional `Viewport.row_copy`, with exactly one record per visible physical row:
`RowCopy { wraps_to_next: bool, written_end: u16 }`.

- Default vt100 exports actual visible rows at the same parser state as runs.
- `written_end` is the exclusive grid end of the final cell with text data,
  including authoritative width; typed spaces count, empty padding does not.
- Every extent is within viewport columns. A final row's wrap flag may refer to
  a row outside the viewport; a selection never joins beyond its own last row.
- Optional Ghostty exports `None`. Its current physical-copy behavior remains;
  no guessed flags or claimed optional-engine wrap parity.
- Only bounded visible metadata crosses the existing snapshot serialization.
  Do not clone all retained history, retain backend pointers or change read-screen
  contents semantics. Old snapshots own their metadata across feed/resize.

Copy omits a row separator only when a selected boundary is authoritatively soft
wrapped. At that boundary retain actual written trailing spaces and omit empty
right-edge padding/wide-wrap spacer cells. Preserve hard separators/blank rows and
existing explicit same-row edge-space policy. Missing metadata retains physical
row behavior. Define metadata validation before consuming it; malformed metadata
must not silently produce guessed logical text.

## Regression and verification sequence

1. Add independently specified real default-engine width/space/wide-margin/hard-line
   metadata regressions, including resize refresh and parked history. Retain the
   actual focused command's intended red before implementation where applicable.
2. Extend the existing finite real PTY fixture with a bounded private control phase
   after viewport/requested-size convergence. The child observes actual kernel
   columns before emitting a known wrapped payload; record that independent width.
   Preserve the original alpha event and18 accepted cases. No fake view, selection,
   focus or outcome injection.
3. Commit/push the native regression on the isolated diagnostic branch and retain
   actual wrapped-copy red on the existing selection owner before publishing fix.
4. Export metadata and implement native copy joins. Preserve hard/blank lines,
   written soft-boundary spaces, wide-margin behavior and exact NFD bytes. Exercise
   current parked history through actual native history input, not injected offsets.
5. Require same-case native green and existing18-case preservation, real app/PTY
   identity, exact retained sources/archive, shared shutdown and independent review.

Reuse unchanged Rust/C ABI baseline results; run focused adapter tests/Clippy for
metadata changes and actual native compile/copy gates for Swift/fixture changes.
The parent owns current-main publication and other active hosted workflows.

## Status

Diagnostic implementation accepted. Native intended red is independently accepted
at8e96b17/run38837076163; all18 original cases passed before the19th exact
wrapped-copy mismatch. The production metadata/copy repair and expanded24-case
fixture pass actual native run38840412891 at353a97d, including production decoder
compile/contracts,24 exact UTF8 clipboard cases and awaited shared shutdown.
Independent source and artifact reviews pass. Focused11 real adapter tests, strict
Clippy, actual session history and six-phase portable PTY transport also pass.
The intermediate5a71574 run38839319267 was an actual app build success followed
by a standalone harness dependency failure; corrected actual dependency linkage
is included in353a97d. Parent current-main integration/exact-main acceptance
remain pending. Optional exporter and broader selection gaps remain below.

The parent published-main12d5c14 run38836229618 separately passes the existing18
visible-selection cases, with exact235-source-copy artifact review. That does not
accept any of this new wrapped-copy WIP.


## Accepted intended native red and prepared owner repair

Native run38837076163 at diagnostic8e96b170fd5cdc2b1ff0cdf076c6a0af0fe3768d
passes all18 original exact UTF8 cases and fails only the appended wrapped-copy
case: expected99 X characters plus U+0020 plus EDGE; observed99 X characters plus
U+000A plus EDGE. Actual app/activationPID6207 and childPID6211 parent/nonce match;
focus is active/visible/key/terminal responder, kernel/requested/viewport31x100.
All225 retained source copies match Git8e96b17; archive/source plist agree.
Buildexit0/nativeexit1 in6.989s, no timeout. Artifact12019986394 ZIP SHA256
`1c15d6564b5419d4c02b6e87b701bf5661fb89426bcea0860d6a6b30b621b05d`.
Independent intended-red review SHA256
`699e7b13b9106cf5029230bcff88f1aa70d7a4419158229146bf9c3d160930f9`.

Prepared production metadata comes from the same immutable worker row walk as
runs and preserves exact written-space extents. Native decoding rejects present
metadata if viewport bounds, drawing/copy row counts or written extents disagree.
Optional metadata remains unavailable explicitly. Native copy joins only actual
wrapped boundaries; empty physical rows retain separators. Existing same-row edge
spaces and physical triple-click bounds remain. The repair is published diagnostically
at5a71574; parent current-main integration remains separate.

Focused real vt100 tests pass across widths4/7/31, written boundary spaces,
wide-margin written extent, hard/blank rows, actual parked history, output while
anchored, resize refresh, retained snapshot ownership and alternate separation.
All11 adapter parsing tests and strict adapter all-target Clippy pass. The native
runner also compiles/executes the actual production Swift Viewport decoder against
valid/unavailable and malformed count/extent/grid-bound inputs; that new Swift
wire assertions remain unexecuted after the first hosted probe compilation failure.

Prepared native green retains original18 plus the unchanged intended-red case,
then full-edge wide cell, combining/emoji boundary, hard/blank lines, explicit
wide-margin engine policy and actual Command-Home parked-history copy. It retains
actual row metadata for each phase, matches existing accessibility contents to the
captured viewport, and preserves strict byte oracles. Six-phase actual owned Linux
PTY transport passes independently, receipt SHA256
`9fcbf47f597c77e452a29217158ed3f973d4d67dcd106bd8f395ceeb94831db2`.
It does not establish native geometry, metadata decoding or clipboard behavior.

## Default-engine wide-margin policy gap

Real tests expose vt1000.16.2's intentional policy when a wide glyph moves because
only one empty column remains: row_wrapped=false and contents keep a hard LF.
Public engine source documents that choice. The parent explicitly directed
preserving it; no cursor/width heuristic manufactures a wrap and no upstream
source or pin changes. The metadata still distinguishes the shorter written
extent and the next row's authoritative wide-cell span. A full-width wide cell
followed by another character does have an authoritative soft-wrap boundary.

Full wide-margin logical-join parity requires a separate engine-policy decision,
including whether an optional F04 exporter can supply true line-wrap and written
extent metadata within its resource contract. Current optional RawFrame does not.
Do not claim that gap, persistent selection anchors, autoscroll, wrapped-word
expansion, rectangular selection, copy mode or mouse protocols closed here.

The diagnostic slice is accepted after actual decoder/contracts and native24-case
green/shared shutdown. Parent current-main publication and exact-main acceptance
remain separate.


## First repair run: harness dependency failure

Run38839319267 at5a715747b18df8370264ce443844dac8ec462b91 built the actual
app successfully (exit0,263.074s), then the standalone decoder probe compilation
failed (exit1,4.232s). Compiling Models.swift alone omitted its actual production
notification, window and browser model dependencies. No native app/clipboard
phase ran, so this is a harness boundary failure, not a native copy result.
All226 retained source copies match that exact Git commit; app plist/source and
archive agree. Artifact12020617097 ZIP SHA256
`e1fede116b7ac528f0cd00e4405e7d9018d3007cfe5708a49091dde216c30394`.
App archive SHA256
`4727eb138d78fc30a3ca36efba417975da2c2f4f787b05b679b6c3afd57239df`.
Failure/source audit SHA256
`251b0c05e805e14f5da67660613b126055b96be8203b1ff7288597bd0660be54`.

The narrow harness repair preserves the production repair, independent decoder
assertions and all24 exact clipboard cases. It compiles the actual eight-file
model dependency closure with the real C header, links the just-built app bridge
with an absolute rpath, verifies that bridge against the archived app, and records
the explicit compiled input hash subset plus bridge hash. All dependency sources
were already in the retained all-app/crates inventory. No fake model definitions
or copied decoder are introduced.


## Accepted corrected native wrap green

Run38840412891 at353a97d63fd05ea765500d829df896978f174813 passes the actual
app build (278.034s), real production decoder dependency compile (17.9s), decoder
contracts (0.062s), and actual native selection (8.045s), all exit0 without timeout.
The decoder accepts valid/missing/null metadata and rejects all five independent
malformed count/extent/grid-bound inputs. The probe's ten actual source/header
inputs are retained and hashed; its linked bridge equals the actual archived app
bridge. No production decoder or dependent type is replaced.

All24 exact UTF8 clipboard cases pass through actual mounted view events and menu
Copy. The unchanged intended-red19th case now retains99 X characters, U+0020 and
EDGE with no inserted LF. Full-edge wide, combining/emoji, hard/blank and actual
parked-history cases also pass. Actual app/activationPID14446 and childPID14451
match parent/nonce across all six raw kernel phase observations; requested,
viewport and kernel are31x100, and active/visible/key/terminal responder are true.
Authoritative phase rows retain wraps and written extents; parked offset is8.
The wide-margin policy case explicitly preserves false/99+3 written extents and
hard LF. That engine-policy gap remains open.

All226 retained sources match Git353a97d exactly; archive plist/source agree.
Artifact12021162840 ZIP SHA256
`e67b5579bfacfd6fbbd2985dd4acd3d58a253092dc8e334c6eefde1892d9cf2e`.
App archive SHA256
`e6f63415bc3d4d69be0dfec33a5cddfc91b924d35917c5ffea56378d62702444`.
Actual archived bridge SHA256
`1e8608e35006910b05b4f35172348538f68bdd321d474d90139bb1661aacf447`.
Child source/artifact audit SHA256
`62ea13da90f8138741f1db9538c6033393453372f9dfa4b571728481ead86822`.
Independent artifact review SHA256
`59c8bb2c496ad10f23a4ce5d2584a63b080d2d0a4f47cf111bce80e4407b2702`.
The earlier narrow harness source review SHA256 is
`1dfb988f1a434d0afb1dd74b34e0fcee217745fd831a4ce805e65f108e3cce8b`.

Complete is saved after awaited actual shared shutdown and scoped owned app-group
cleanup. No separate final PTY-child absence ps sample was retained. No physical
pointer, VoiceOver, optional-engine metadata or broader full-parity claim follows.
The parent owns current-main integration and exact-main validation.
