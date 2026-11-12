# Ghostty fixture final recheck

**Date:** 2026-11-12
**Scope:** The owner-fixture correction for the mixed-order and ST findings.
**Verdict:** Accepted for this scoped fixture correction. No remaining findings.

## Recheck

- The legacy feed remains one combined input containing OSC 9, OSC 777, BEL,
  DSR, and RIS (`\x1b[6n\x1bc`). Its assertions retain the original first five
  event kinds `[4, 4, 1, 2, 3]` and require the complete sequence
  `[4, 4, 1, 2, 3, 4]`, followed by the RIS metadata record `(133, 2, 0, 0, 0)`.
  This preserves the DSR reply in the same feed and checks that RIS metadata is
  appended after the legacy reset event.
- The new single-feed case asserts the exact prompt/BEL/ST/RIS event tuples
  `[(4, 0, 0), (4, 1, 0), (4, 2, 0), (3, 0, 0), (4, 3, 0)]` and metadata
  record tuples, including source, subcommand, status, terminator, offset, and
  length. It separately compares the BEL and ST payload bytes and checks that
  title, cwd, notices, and notice sequence are unchanged.
- The ST-positive case uses `ESC \\`; its record assertion requires status 0,
  terminator 27, and the exact `state=blocked:id=st` payload. The owner accepts
  this terminator in the same branch as BEL.
- Callback ordering agrees with the supplement: prompt handling runs the
  existing semantic-prompt callback before emitting source 133/subcommand 1;
  RIS runs the existing reset callback before emitting source 133/subcommand 2
  (`tools/terminal-feasibility/program-status-v0.4.patch:211-236`). The C
  collector records reset as event kind 3, while `metadata_event` appends kind
  4 with the current record index before copying its payload
  (`collector.c:35-39`, `metadata.c:15-34`). On successful commit,
  `integrated_commit` clears collected events and `metadata_commit` clears
  record and byte lengths (`integrated.c:128-133`, `metadata.c:74-78`).
- The patch expands the former event assertion while retaining its original
  five-event expectation and surrounding notice/title/cwd/reply assertions.
  It does not split RIS into a later feed, so the prior DSR reply is not lost.
- The fixture patch's SHA-256 is
  `1aeaedaf7004dbc2af5ff618d2c3c566f12ece2515c8a29d127b3e039a181e33`, matching
  the fixture entry in `tools/terminal-engine-artifacts/candidate-sets/ghostty-7501-v1/source.json`.

## Evidence limits

This was a source and assertion review; I did not rerun the fixture or
`scripts/check`. The parent reported 47 Python tests (46 passed, 1 skipped),
preflight success, and isolated Rust metadata compilation success; the full
check was still running during this recheck. No Darwin producer run, native
Ghostty callback execution, or accepted linked-library result is claimed. A
fresh Darwin producer remains the native callback gate.

Only this review record was written. Source, test, index, and staged state were
left untouched.
