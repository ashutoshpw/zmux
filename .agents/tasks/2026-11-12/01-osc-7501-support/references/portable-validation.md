# Stable integrated portable validation

`RUST_TEST_THREADS=2 scripts/check` passed on the complete OSC 7501 source
checkout before its integration commit. Current HEAD was `d573e8b`; this is a
validated dirty source set, not a claim that HEAD alone contains the feature.

- Rust results: **756 passed, 0 failed, 7 ignored**.
- Formatting, workspace/all-target strict Clippy, license inventory and native
  bridge C ABI/profile lease checks passed.
- 139 resolved packages, 108 notice texts; the three upstream-review entries
  are pre-existing and remain explicit.
- Runtime/native/embedded payload/manifests and relevant script/tool source
  fingerprints were identical before and after execution.
- Unrelated profile-lifecycle preservation hashes matched before and after.
- Duration: 282.19 seconds. Full log and detailed fingerprint receipt:
  `/tmp/zmux-osc7501-final-check.log` and `.json`.
- Artifact policy suite separately passed 43 with one Apple-linker skip; recipe
  preflight verified 149 inputs. No Ghostty library was built by that preflight.
- Native Swift/AppKit and actual linked Ghostty acceptance remain hosted gates.

Earlier failed full-run receipts were retained separately; this pass includes
the SSH bootstrap, native-frame fixture, checkpoint, route-retirement and
graceful-close repairs. Final scoped review and source commit attribution are
recorded in task status when completed.

## Final hosted-repair source validation

The combined route-budget, final-admission rollback, Swift receipt casts and
versioned fixture source set passed `scripts/check`: **757 passed, 0 failed,
7 ignored**, 277.16 seconds. All strict static, license and ABI checks passed.
The before/after source fingerprints and unrelated preservation hashes match.
The final fixture Python suite ran 47 total: 46 passed, 1 Apple-linker skip;
its staged Rust metadata compile and recipe preflight passed. Native compiler,
producer, actual callback execution and app acceptance remain hosted gates.
The detailed latest fingerprint receipt is `/tmp/zmux-osc7501-final-check.json`;
previous successful source receipts are archived as `check-round4` through
`check-round7`. Final source commit attribution follows in task status.

## 2026-11-13 final receipt-bearing Event repair

`RUST_TEST_THREADS=2 scripts/check` passed **759/0/7** in 282.22 seconds. The
receipt and log are `/tmp/zmux-osc7501-final-check.{json,log}`; the previous
759/0/7 provider-state-only receipt is archived as `check-round10`. This run
includes the absolute receipt-Event control path, unknown outcome handling,
stronger real-PTY/public-execute tests, and the Ghostty existing-method repair.
Runtime/tooling fingerprints remained unchanged; unrelated work matched its
preservation ledger. This is local source validation, not macOS/linked execution
or packaged application acceptance.

Final parent response-classification repair recheck: **759/0/7** in 277.19 seconds, unchanged source fingerprints and matching unrelated preservation hashes. This supersedes the previous local receipt for the final runtime slice. Hosted gates remain separate.

Combined Darwin progress/fixture repair full check: **759/0/7** in 286.36 seconds, source unchanged and preservation matched. The preceding 277.19-second runtime-only check is archived as `check-round12`. Optional Ghostty execution remains a hosted gate.

Final fixture/diagnostic and cleanup-proof full check passed **759/0/7** in 264.34 seconds, source stable and preservation matched. Previous 270.30-second receipt is archived as `check-round14`. Native workflows remain separate acceptance gates.

Final retirement-owner and fixture aggregate passed **763/0/7** in 262.13 seconds, including full C ABI/native lease checks, source unchanged and unrelated preservation matched. Prior failed aggregate is archived as `check-round18`; no root cause is inferred for its isolated lease assertion.

Short Unix-socket checkpoint fixture aggregate: **763/0/7** in 285.75 seconds, all static/license/C ABI gates passed, source unchanged and preservation matched. The preceding complete 262.13-second receipt is archived as `check-round19`.

Provider snapshot/read and final-drain handshake fixture aggregate: **763/0/7**
in 283.08 seconds, all static/license/C ABI gates passed, unchanged source
fingerprints and matching unrelated preservation hashes. This validates the
uncommitted fixture changes on `c346051`; prior complete source receipt is
archived as `check-round20`. Post-fix hosted/native acceptance remains pending.

After removing the review P3 private dirty-result assertion, final aggregate
passed **763/0/7** in 282.56 seconds, source fingerprints unchanged and unrelated
work preserved. Prior complete 283.08-second receipt is archived as
`check-round21`; current receipt/log remain `final-check.{json,log}`.
