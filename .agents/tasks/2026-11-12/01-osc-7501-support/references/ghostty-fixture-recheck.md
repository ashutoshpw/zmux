# Ghostty fixture repair recheck

**Date:** 2026-11-12
**Scope:** versioned Ghostty metadata fixture repair only
**Verdict:** two scoped test findings; no pinning or staging implementation defect found.

## Findings

1. **P2 — The fixture does not assert callback order in a mixed input stream.**
   In `tools/terminal-feasibility/program-status-v0.4-owner-fixture.patch`,
   the old combined fixture input has its trailing RIS removed (hunk near line
   20), then RIS, prompt, and raw 7501 are fed separately (near lines 30, 67,
   and 90). The new assertions verify each callback tuple and state isolation,
   Successful `metadata_commit` clears the captured callback lengths after each
   call (`tools/terminal-feasibility/metadata.c:74-78`). No assertion checks
   the ordered callback list for a stream containing prompt, raw 7501, and RIS
   together, or their ordering relative to the existing event sequence.
   A callback reorder or association error across those forms would pass these
   assertions. Retain the existing fixture assertions and add a combined-stream
   case with an exact ordered callback vector.

2. **P3 — The ST-terminated raw 7501 acceptance branch has no positive fixture
   assertion.** The new owner accepts terminator values 7 and 27, but the
   positive raw-report case only sends BEL (`0x07`). A malformed callback is
   tested, but no case proves that ST (`0x1b 0x5c`) reaches the raw callback
   with terminator 27 and unchanged payload bytes.

## Checks and retained contracts

- The patch adds explicit source 133 handling only for subcommands 1 and 2
  with empty data and terminator 0. Source 7501 is accepted only for
  subcommand 0, a nonempty payload, and BEL/ST. The changed match remains
  closed by its rejecting default arm. The added controls reject an unknown
  source, unknown 133 subcommand, malformed 7501 records, and event kind 99;
  there is no new blanket unknown-source or unknown-event ignore.
- Existing owner assertions remain in the patch diff, including the legacy
  notification sources/texts, title, cwd, response bytes, record status and
  size-bound cases. The old mixed input was altered only to split RIS into its
  own callback assertion; that split is the ordering finding above.
- Fixture staging copies pinned `metadata.rs`, `integrated.rs`, and
  `metadata-owner.rs` into one temporary sibling directory, preserving the
  `include!` paths, then applies the separate fixture patch. The producer
  receipt records those source hashes, the pinned base owner, ordered patch
  identities, and the staged patched-owner digest. `accept-linkage.py` binds
  the captured source hashes to the historical pins and validates the receipt
  against the versioned source manifest and exact accepted receipt digest.
  The positive staging test recomputes the patched-owner hash from the staged
  output.
- The historical candidate remains the default route. The versioned candidate
  requires the explicit `ghostty-7501-v1` selection, rejects artifact reuse,
  and has no fallback when its source or linkage manifest is absent. The
  versioned linkage manifest is not present in this checkout, so this repair
  is not accepted linked-library delivery.
- Preserved identities verified in the worktree: historical candidate
  `038aa8eb5dd381c353733550e95579221e1ea6967ae03a860c486436eb690c12`,
  historical linkage candidate
  `595b5db8b04a9c108f614008df7364a78648e843c4d1c3e28350119bd5ff5d6d`, Darwin
  recipe `f57162b68a7866935bbd7662629b7508a4fbd6102134a5cd10efa21010be914d`,
  historical owner base
  `8abf975e8737f093062452f71b006dd5f39927cdfd3402a133026176e4c395d1`, and
  unchanged engine supplement
  `f2ea5e25d0f9015c4118ef864ab8c6c85f64e7285a5f06cad8321516995f4925`. The
  new fixture patch is
  `fe4581f033ae8832be3a4eb1502d6680fa87e22d27308527a8ca16fc106551dd`, the
  versioned source manifest is
  `fc65a45623707b4a9b3301e69698bcb6bf4a952e7f594f301ffa8b9e06734d04`, and
  staged patched-owner output is
  `de0da8f9b9d4ea800234104cccc5773ac29e04709e0f2fe65a10377ebab7c601`.

## Reproduced evidence and limits

- `python3 -m unittest discover -s tools/terminal-engine-artifacts -p 'test_*.py'`:
  **47 total: 46 passed, 1 skipped** (Apple linker unavailable).
- `build-darwin.py --check-recipe --candidate-set historical`: passed with 148
  frozen inputs. `--candidate-set ghostty-7501-v1`: passed with 150 frozen
  inputs and the exact source manifest hash above. Both report
  `engine_built=false` and `darwin_execution_verified=false`; the versioned
  recipe lists only the three unchanged engine patches.
- Reapplied the fixture patch in a temporary sibling directory and compiled
  its staged entrypoint with `rustc 1.95.0 --edition=2024 --crate-type=lib
  --emit=metadata`: passed; staged entrypoint and both sibling includes were
  present, and the receipt output digest matched the staged owner bytes.
- This host is Linux. No macOS ARM64 producer run, linked callback execution,
  successful `metadata-native-arm64` result, accepted `linkage.json`, or
  independently accepted libraries were available. Fresh production using
  this exact source manifest remains required. `scripts/check` was not run;
  the review did not modify source or tests. No source/index state was changed.
