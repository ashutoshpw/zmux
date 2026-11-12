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
