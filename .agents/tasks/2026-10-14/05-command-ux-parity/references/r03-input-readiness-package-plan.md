# R03 separate readiness package proposal

2026-10-16. Read-only source discovery and proposed contract. No readiness/package
flag combination has been enabled, no package has been built, and no diagnostic
has been dispatched for this proposal. The v6 source/binary, old retained package
pin/admission, ordinary acquisition and native defaults remain unchanged.

## Recommendation and execution boundary

Use one fresh Darwin ARM64 job after the observer4 consumer source freezes. Build
source plugins with the existing unchanged plugin owner, add the already reviewed
readiness patch, and run the full public consumer owner plus the actual silent
readiness owner against the exact produced executable. Construct a separately
named readiness package with all corresponding source and existing reviewed
notices. Keep it private to that job. Do not upload executable/package bytes.

The runtime agent confirms its diagnostic interface is
`ZMUX_TEST_ZELLIJ_BIN=<exact produced executable>`, version 0.43.0 plus the exact
readiness capability. The actual public owner suite includes the observer4
Desktop path and completion-before-Ready. It can consume the fresh producer in
the same job, without any change to `package_admission.py` or
`retained-native-package.json`. This is diagnostic runtime evidence, not an
AppKit bundle or retained distributable package admission.

Do not build now: the observer4 source owner is still repairing/freezing a
completion race. Running its complete owner once after that freeze avoids a
second package build solely because consumer source changed.

## Existing owners and gaps

- `produce-patched.py` already owns pinned source extraction, environment/cache
  constraints, EINTR red/green, optional plugin rebuild, Wasmtime validation,
  actual compiler-unit capture, root build, unchanged lock, capability and
  readiness reporter controls. The current readiness/package exclusion is an
  explicit guard; changing it alone is insufficient for a legal source package.
- `package_build.py` owns exactly ten shipped source-built plugins, their actual
  compiler-unit evidence, hashes and pinned Wasmtime validation. Reuse these
  functions and manifest unchanged. A fresh package needs source-built bytes;
  the current prototype binaries embed original upstream prebuilt WASM.
- `run-patched-owner.py` / `owner_contract.py` already bind exact binary, source,
  logs and executed public behavior. The integrating owner must additionally
  establish that observer4 behaviors actually ran, not infer this from a count.
- `package-patched.py` already requires a source-built producer and full owner,
  independently dumps embedded plugins, retains notices/sources/compiler
  evidence, and hashes the complete payload. It currently has no mandatory
  readiness owner, no readiness variant identity, and retains only the EINTR
  patch in corresponding source.
- `package_sources.py` deliberately keeps the filtered upstream archive pristine
  apart from the existing documented WASM exclusions/materialized license links.
  `package_notices.py` adds the EINTR patch and build instructions separately.
  Preserve those transformations; append original readiness source/patch/protocol
  beside them instead of rewriting the filtered upstream archive.
- Legacy `package_admission.py` and `retained-native-package.json` admit one exact
  reviewed old package. Leave this tuple and its existing callers untouched.

## Proposed production edit footprint

1. **`produce-patched.py`**: remove the readiness/plugin-rebuild combination
   rejection only after source review. Both explicit flags are required for the
   readiness package producer. Existing plugin rebuild/validation and compiler
   capture execute once unchanged. Preserve normal producer and prototype modes.
   The input-readiness inventory must be refreshed because this producer hash
   changes; schema and all four actual upstream source hashes stay identical.
2. **`package-patched.py`**: add explicit `--readiness-owner-receipt`. A built
   readiness variant requires it; a legacy built variant rejects it. Verify exact
   producer receipt, executed binary, original schema/capability and four-file
   source inventory, successful silent-owner source/log hashes, accepted3/EOF,
   same live retained frontend, two unique writes, startup count one and cleanup.
   Existing full owner and rebuilt-plugin gates remain mandatory. Keep legacy
   package names/manifests unchanged. The readiness manifest adds a distinct
   `input-readiness-v1` variant, schema/inventory and readiness-owner digests;
   its archive name includes that variant and inventory prefix to prevent any
   collision with the retained legacy package.
3. **`package_notices.py` / `package_sources.py`**: conditionally retain the exact
   original MIT reporter, patch 0002, protocol and source inventory under a
   readiness source directory beside `SOURCES/stdin-eintr.patch`. Retain Zmux and
   upstream MIT texts as today. Add exact patch/copy/capability instructions to
   `SOURCES/BUILD.md`; update source-availability descriptions for this variant.
   Verify every added member against the built inventory and verify applying the
   retained patch/copy to the filtered source reconstructs the four compiled
   hashes. Keep the fifteen upstream WASM exclusions, registry exclusions,
   pristine MPL archives, notices and policy logic unchanged.
4. **`check-package-gates.py`** (or one narrowly owned readiness-chain control in
   the same module): establish a successful real readiness package/owner chain,
   then reject missing/mismatched readiness receipt, protocol/inventory/source
   mismatch, missing corresponding reporter/patch, altered readiness semantics
   and wrong variant. Exercise the package/archive boundary, not mirrored private
   predicates. Reuse existing generic plugin/source/notice contracts; do not
   create another plugin test inventory or regenerate their established policy.
5. **Workflow**: extend the already reviewed explicit readiness call with a
   separate package mode and exact existing notice-policy hash input. False
   preserves the current dependency diagnostic. Package mode requires both exact
   source review and policy review, runs the existing source-built plugin path,
   frozen full consumer owner and actual silent owner, then prepares the distinct
   package. Keep package bytes ephemeral and upload source/evidence only. No new
   legacy artifact selection or native default. Workflow source gets a separate
   review before a single named run.
6. **Inventory/docs**: refresh the producer input hash and record one new reviewed
   source inventory, edit footprint, exact run source and package manifest/owner
   hashes. Preserve all existing v6 / prototype receipts as immutable evidence.

## Required package contract

The package must bind one exact executable across producer, full consumer owner,
actual readiness owner, package manifest and dumped embedded plugins. It must
retain the complete applicable legal/source set and original new integration
source, not merely assert capability or add an archive-name label. Current notice
policy remains unchanged because this original MIT patch adds no third-party
package or upstream Cargo dependency. The previous independent policy review is
reused only by its exact hash; it is not a fresh review of unexamined bytes.

The existing source-built plugin/Wasmtime implementation is reused as-is and run
once for this fresh artifact, since prototype WASM bytes do not establish package
provenance. Reusing an old package's dumped plugin bytes instead would require a
new provenance adapter: old compiler package IDs/manifest paths do not belong to
the fresh metadata graph, and blindly relabeling them would fabricate current
compiler evidence. That larger optimization is unnecessary for the first package.
No new plugin schema, notice policy or generic validator suite is proposed.

Keep package promotion separate. The first same-job review package can use the
existing `--prepare-only` mode and consume the exact producer binary for runtime
proof. It must not claim that an `UNREVIEWED-` archive passes the legacy immutable
admission owner. A later actual app-bundle consumer needs a separately reviewed
readiness-package admission tuple/path and a check against the final bundled
binary. Neither changing the old pin nor silently accepting arbitrary packages
is part of this proposal.

## Proposed sequence after source approval

1. Freeze consumer source; review this package-owner implementation and refreshed
   inventory before any new dependency/package build.
2. Fresh Darwin producer with explicit `--input-readiness --rebuild-plugins`.
3. Full public owner (including actual observer4/terminal-before-Ready controls)
   and actual silent readiness owner against that exact produced executable.
4. Prepare the distinct package with both owner receipts and complete source/
   notices; inspect manifest, dumped-WASM identity and focused negative controls.
5. Retain source/evidence and report diagnostic runtime proof separately from
   package publication, final app relocation/bundle testing and default selection.

These are proposed commands/interfaces, not current execution evidence. The
present producer still rejects the combined flags and the packager still lacks
`--readiness-owner-receipt`.

## Implemented for source review (not built)

The parent approved the proposal at SHA-256
`05c559c5f28d681f6f021de8697fec69b65c6d52e051bfc008cb54f25209f04e`.
The conditional owner is now implemented locally with shared production contract
module `readiness_package.py` and a focused real-archive gate tool
`check-readiness-package-gates.py`. Both are consumed by the package/workflow
production path; no test-only production export was added. Generic plugin and
legacy package admission owners remain unchanged.

The focused controls protect independent contracts absent from the generic
package gates: actual readiness owner and EOF proof; named executed observer4
behaviors plus semantic receipts; corresponding source in a self-consistently
rewritten real archive; exact protocol; distinct variant. Credible regressions
are accepting transport progress, a substituted test with unchanged count, or
shipping an executable without its original reporter/patch. Positive controls
require a real new producer/full owner/silent owner/prepared archive. These
controls have only been parsed; no synthetic success or fabricated red is claimed.

The full consumer semantics are pinned by the runtime owner's three named tests
and the actual `R03 typed input readiness` / `R03 completion before input
readiness` receipts for exit0/7. Package execution remains held for the consumer
source review/freeze and the separate package source review.
