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

`TerminalSelection.text` currently trims crossed rows then inserts a newline at
every physical row. `Viewport` has runs and aggregate history offsets, with no row
wrap/identity metadata. `TerminalView` clears selection on explicit history wheel,
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

Design approved; optional-engine unavailable metadata disposition coordinated.
Regression-only real native phase is prepared; no copy/metadata repair exists yet.
An actual native w key reaches the finite raw PTY child, which records kernel
rows/columns and emits a known wrapped boundary-space payload. All18 accepted
original cases and the exact alpha event precede the new intended-red case.
Actual focus, child identity/nonce, requested grid and kernel viewport must agree
before event/copy delivery. The actual wrapped clipboard mismatch remains pending.

Portable real owned PTY phase transport passes with actual kernel31x100, exact
payload bytes and PID/PPID/nonce; direct owned child terminated/reaped safely.
Receipt SHA256
`1d9ac35e4ac0d56dab7a225625679eefe1d9b67d3b0250a65e148145daa3931e`.
This is not AppKit/native clipboard proof. Parent exact-main selection38836229618
passes at12d5c14; its source/artifact audit is separate from this new regression.
