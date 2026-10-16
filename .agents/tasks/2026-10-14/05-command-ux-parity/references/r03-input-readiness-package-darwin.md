# R03 combined Darwin package attempt

2026-10-16. Run [38852664400](https://github.com/ashutoshpw/zmux/actions/runs/38852664400)
executed exact integrated source `9a42f8510d84bb8b757e71cba1ff8ca45d134b5e`,
using the reviewed inventory
`8047ccc7c75929c94c5551ef600081fd27eea501b03ece17785af02f095ffb51`.
The workflow failed in public-owner receipt verification. Package preparation
and focused archive gates did not run. No package or executable bytes were
uploaded/retained, and no admission/default activation occurred.

## Retained evidence

Source/evidence artifact `12026471948`, ZIP SHA-256
`50071fc77b7bc03c013cfaa924026542bb4c5c9dcd3f609ac89f94e10257fba9`.
Local audit: `/tmp/zmux-input-readiness-package-darwin-38852664400/failure-review.json`,
SHA-256 `f0175edb51061380e1990f37a4da4adab9df4043ac297b4ba7d6a6ddcb5f2769`.
The audit verifies retained repository bytes against exact Git source, all four
compiled upstream hashes, complete executed owner source inventory, raw native/
plugin compiler units and embedded-assets manifest, and all receipt/log hashes.

- Producer receipt:
  `ea984220c94183077e44bc7c34d0f927144c8779be2983e084faae8c8fe435aa`.
- Actual silent readiness owner receipt:
  `2f421dcd51d3b9d9cfb3d320e5f0df91fb3924ddaa1d1488baf02f3b3d9f35fe`.
- Failed full public owner receipt:
  `932ceb1c9ea82f6f74c2f79821babce7893cc3cbac1c9b1b4e5b24aeefa598ad`.
- Recorded executed binary (bytes deliberately absent):
  `8c4f10864b52f7be16ae8126d50be0742a2e9680e0804625c7755b3593b91bfc`.

Ten source-built plugins, existing Wasmtime validation, seven reporter controls
and actual silent owner passed on macOS15.7.9 ARM64. The silent owner observed
two exact one-write first lines, distinct retained live frontends, accepted3/EOF,
startup count one, exit0/0 and backend/workload cleanup.

Every public-owner command exited0, with no timeout or truncated log. Counts:
owner-0 `2/15/3` passed with zero ignored; owner-1 `14` passed with one ignored;
owner-2 `3/22` passed with zero ignored. Named observer4 actual behavior passed:
typed input readiness, distinct attachments, one first line each, startup1,
actual saved exit7 and joined frontend. Completion-before-Ready exits0/7 both
saved authentic completion and joined the frontend without input admission.
Existing finite exits0/7/143 receipts also passed. These are Rust Desktop/CLI
consumer receipts, not AppKit start/attach or final app-bundle acceptance.

## Exact failure and repair boundary

The broad owner-1 `local_zellij` filter included
`local_zellij::input_readiness::tests::retained_readiness_peer`, an intentionally
ignored owned subprocess entrypoint. The real parent protocol tests spawn it
with `--exact --ignored`. The unchanged strict no-ignored semantic verifier
correctly rejected the broad outer enumeration with
`RuntimeError: owner suites missing, failed or ignored`.

The parent approved excluding only that fixture entrypoint from outer selection,
using one shared command list for patched and native package owners. Preserve
strict no-ignored verification and require all five real protocol-parent test
names in the owner contract. The frozen Rust source and actual behavior stay
unchanged. Focused Linux validation of the repaired shared production command
passed all14 actual tests, zero ignored, including all five fixture parents.
Log SHA-256:
`09d580ef5df0e63ece197d583e9280d9e785374011356f36f77e68f4c2d1bffc`.
This is a focused harness green, not a rewritten successful Darwin wrapper.

The first promotion freeze at `fdfb786` also omitted refreshed package/helper
hashes in its inventory. No run used it; producer validation would reject it.
That source approval is superseded. The repaired freeze must verify every one
of the18 inventory input hashes against exact Git source, with protocol and
four compiled upstream hashes unchanged. Old evidence remains bound to its
original8047 inventory. A parent-reviewed fresh combined run may exercise the
complete producer/consumer/package/gate chain and finite diagnostic retention;
there is no successful package evidence from this failed attempt to reuse.
