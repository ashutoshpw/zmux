# F04 static artifact provenance and notices

2026-10-15. Approved bounded prerequisite after worker-owned startup and isolated
engine/PTY experiments. New original tooling belongs in
`tools/terminal-engine-artifacts/`; no default-engine selection, Cargo/app linkage,
pin/patch change or redundant engine feasibility rebuild.

## Scope and source boundary

Inventory the exact accepted static library, its producer receipt, source/patch/
header/shim/toolchain inputs and actual required runtime notice material. Preserve
verbatim permissive notices. Distinguish linked code/data from build-only packages;
missing or unsupported notice material must fail acceptance rather than inherit a
root MIT label. Produce a versioned hash-verified manifest with explicit target and
architecture scope, not a portable/universal/runtime claim from a host archive.

Only these source families are authorized for this slice:

- Ghostty `22fa134a07f5749edd9e92c6bc17d8aaf61843be`, existing pinned archive SHA
  `a07afea0e0b01cdfbf5b418c56bfd67366929dac0f9636267de5e5637351df5f`, root MIT.
- Original unchanged Zmux resource/metadata patches and C shim/header inputs.
- Zig 0.16.0 host archive from existing verified pins; inspect bundled runtime
  component notices separately from its root MIT license.
- Full uucode commit `9d55524551411b493cca41ca06363625d90aff1e`, the exact commit
  referenced by that Ghostty dependency manifest: root MIT plus explicitly
  referenced Unicode and Bjoern Hoehrmann material. Record full-archive SHA before
  retention; do not treat the filtered Zig package hash as covering omitted files.

The accepted library is a regular archive containing its main compiled unit and
bundled compiler runtime. Minimal SIMD-disabled/graphics-disabled imports include
uucode and generated Unicode tables. uucode's filtered Zig package omits `licenses/`
while its license and UTF-8 source reference those texts; root-only inventory is
therefore insufficient. Fetched packages are not automatically linked packages.

## Steps and verification

- [x] Retain exact source/archive and referenced-license provenance.
- [x] Document linked/build-only classification with concrete source evidence.
- [x] Implement bounded static-archive inventory and fail-closed manifest/notice
  validation; verify actual object architecture without relying on file naming.
- [x] Exercise retained reconstructed Linux library and meaningful corruption/missing-
  notice/wrong-target failures; reuse the single reconstruction required by the
  actual PTY publication regression, no separate feasibility rebuild.
- [x] Independent review and exact input freeze; report remaining Darwin/native,
  production Viewport/input integration and named-Mac workload gates.

No app distribution or engine switch follows from a successful integrity check.
Explicit ABI/deployment configuration, platform slices, actual native execution,
full production conversion/ordering and performance decision gates remain separate.

## Retained prerequisite and evidence

`tools/terminal-engine-artifacts/README.md` documents the commands and the concrete
minimal build-graph evidence. The versioned candidate pins the actual reconstructed
library, complete producer receipt, focused real-PTY witness, 32 repository inputs,
upstream and patched header hashes and the notice inventory. Original lost-library
hash/witness information remains explicitly historical. `inspect` cannot accept a
package; `verify` requires all reviewed hashes and rejects unresolved provenance.

The regular archive is 13,889,816 bytes with two actual ELF64 x86_64 members:
main compiled VT unit and bundled compiler_rt. Actual symbols include the patched
metadata constructor, UBSan handlers and cos/log/sin/sqrt/tan; this supports retaining
runtime-port notices rather than treating the Zig executable as the only licensed
component. Archive member paths are recorded but never extracted or executed.

The single reconstruction needed by the PTY publication regression is retained in
ignored `target/terminal-engine-artifacts/20261015-publication-boundary/`, including
exact verified source archives, patched source, toolchain, five C objects, library
and producer/focused receipts. Actual library SHA-256:
`bdc3fc3c6ec69875fed19c220082412b2299bb2fce75cc4e1f49b7fc8673d891`.
Producer receipt SHA-256:
`cefd9ea053b59abf33ddc6433ba759be3e374ece27fa7b6f5dad2924a9e73aaf`.
The active manifest SHA-256 is
`038aa8eb5dd381c353733550e95579221e1ea6967ae03a860c486436eb690c12`.

80 verbatim notice files are retained with source URLs/member hashes: Ghostty MIT;
uucode MIT/Unicode V3/Bjoern Hoehrmann; Zig MIT; exact bundled musl COPYRIGHT;
compiler-rt licenses at all explicitly referenced LLVM revisions; conservative
musl 1.1.24 and 1.2.5 math-file notice blocks, two exact historical log/logf source
revisions, and pinned Go BSD plus its tan/Cephes source notice. Only referenced math comments were retained,
not the math implementations. Musl COPYRIGHT assigns root MIT terms to unannotated
original files; those source hashes/classifications are recorded separately.
The Go tan source itself attributes Cephes release 2.8 and quotes its free-use/no-
support terms; those comments are retained too. Root Go BSD alone does not cover
that complete attribution chain. The unversioned source references prevent a precise origin-revision claim for every
port; conservative coverage was independently reviewed against the observed references
as recorded below. No exact historical derivation or root-MIT-only claim follows.

Focused verification:
`python3 -B -m unittest discover -s tools/terminal-engine-artifacts -p test_verify.py -v`
passes ten tests using real compiler/ar objects, the actual retained candidate
and clones of the real notice
corpus. It rejects wrong architecture/platform, truncated/thin/oversized archives,
a named pipe without waiting for a writer, a valid substitute archive,
missing/changed Unicode notices and deleted inventory
entries. The actual retained library passes structure/architecture inspection and scoped
integrity acceptance. An altered unresolved-provenance fixture still rejects that
same valid archive. The two real-candidate tests explicitly skip on a host without
the retained binary; such a skip establishes no artifact acceptance.
Validation receipt:
`target/terminal-engine-artifacts/20261015-publication-boundary/artifact-validation-reviewed/receipt.json`,
SHA-256 `86dd2afbede0183d8d6c629f45138c9e56676d4461cd71358b88e6e1a4f70375`.
All these checks are prerequisites only. There is no engine selection, production
Cargo/AppKit linkage, Darwin artifact acceptance or workload result in this slice.

## Independent notice coverage closure

The independent read-only review receipt is retained verbatim at
`tools/terminal-engine-artifacts/evidence/notice-coverage-review.json`, SHA-256
`1ded1f188e686a852363a9944f587facf2d47bc92f691e34cc2caa85f7b397e0`.
It identifies no missing notice/copyright for the two requested coverage gates:
(1) unversioned musl/Go port references against the retained conservative releases,
exact historical logs, root classifications and 37 Zig source hashes; (2) Go tan's
complete Cephes copyright/free-use comments beyond the root Go BSD terms.
The author source/notice inventory and both exact musl archives were independently
checked. This factual coverage result does not claim exact upstream derivation.

Parent source review finds no verifier blocker and passes the original eight
format/notice failures (`/tmp/zmux-artifacts-parent-tests.log`). Only the two notice
coverage gates were cleared. The updated verifier binds the retained review's
hash, artifact digest and notice/source inventory digests; unresolved coverage
remains a rejection. All ten focused tests pass on this host, including actual
retained archive acceptance and the altered unresolved-coverage fixture.
The final 96-file source freeze is in the reviewed validation receipt above.
Actual CLI verification passes for library `bdc3fc3c...` with its exact producer
receipt; modified and older unpatched archives and wrong targets still fail.
No default engine, production Cargo/AppKit linkage, Darwin artifact/minimum OS/ABI,
native application execution or workload/performance gate was cleared.

## Bounded Darwin artifact recipe

The next manual prerequisite is implemented in
`tools/terminal-engine-artifacts/build-darwin.py` and
`.github/workflows/terminal-engine-artifacts.yml`; operator details are in
`tools/terminal-engine-artifacts/DARWIN_ARTIFACTS.md`. One macos-15 arm64 job builds
exactly two thin libraries using Xcode 16.4/SDK 15.5, Zig 0.16.0, Rust 1.95.0,
ReleaseSafe, two jobs, SIMD false and the same explicit VT feature list. Both
architectures request macOS 14.0. Upstream's native Darwin generic-target override
emits library declarations of 13.0; C shims and linked consumers remain at 14.0,
matching the existing native app minimum. This split is explicit in the separate
deployment policy and preserves the unchanged actual compiler recipe.
No Linux rebuild, feature matrix or universal assembly is added.

The shared bounded archive reader feeds actual Mach-O CPU-subtype and macOS
load-command checks. Generic arm64/x86_64, exact library minOS 13.0, consumer minOS
14.0 and SDK 15.5 or an
explicit unspecified object SDK are required. The actual SDK/toolchain is checked
separately and recorded; an unspecified object SDK is never promoted into proof.
Both C/extension and Rust metadata consumers compile/link for each architecture;
only arm64 executes. The x86_64 runtime remains explicitly unexecuted.

The driver verifies all 32 source inputs, patched headers and the actual Darwin
Zig archive's 37 reviewed runtime-source hashes. Required exports and defined/
undefined symbols are recorded, including deltas from the actual retained Linux
629-symbol baseline. Completed libraries are immediately copied/hash-recorded;
workflow failure retains slices, objects, fixtures, headers, notices and logs.
Exact source pins, both raw patches and original tooling/C/Rust inputs are bundled
under `sources/repository/` with their full commit/hash map, so checkout loss does
not erase patch contents. Explicit reuse refuses changed recipe/SDK/header/archive
identity and avoids repeating completed engine builds after a later fixture failure.

Local acceptance uses 17 passing Python tests, including real Clang-produced
arm64/x86_64 Mach-O archives and real lower-minimum/wrong-SDK/iOS/specialized-CPU
objects. Damaged bytes are negative cases only. Recipe validation passes 136
hashed inputs with no engine build. The original Linux candidate still passes
integrity acceptance; the new C ABI consumer also compiles/executes on Linux
against that actual patched archive. An off-host driver invocation refuses before
any download or process launch. These local checks establish no Darwin engine build
or native execution. Receipt `/tmp/zmux-darwin-artifact-author-20261015/receipt.json`,
SHA-256 `72e12ba60e8c7f18b5af66f8bf186a1086650fbe55238cf530c6dd5e7b2c162b`.

Hosted execution and review of the resulting Darwin artifact bytes/symbol delta
remain required. The current candidate manifest still accepts only its reviewed
Linux archive. No bit-identical second build, macOS 14 execution, Intel execution,
production AppKit/Cargo linkage, Viewport/input integration, named-Mac workload or
default-engine decision follows from recipe validation.

The first actual run `38797303343` at `adf51320a80e10db85bd8c9b2c01e1e42d268dbd`
compiled arm64 in 93.8 seconds, retained it, then correctly rejected the original
exact-14 assertion. Both actual Mach-O objects declare 13.0/SDK unspecified.
Unmodified failed receipt SHA `75bd83078d904c5d91582b0d65f6e213bee48f9c528cf4670d170239f64d2354`;
arm64 library SHA `df2aa776d0637f13e6641ac443695c9ea4cd592416b9b1a35bf2e73be9c7e658`.
Pinned Config source SHA `e15abd51f767a70c92faac2bc8bf86fbb096db6680bca3de7388b9a4568fd739`
proves the native Darwin query replacement and fixed 13.0 upstream minimum. The
approved repair requires exact 13.0 archive declarations below the consumer 14.0
ceiling, checks linked consumer executables too, and preserves the compiler recipe
for strict reuse. Manual workflow input downloads immutable artifact `12002284112`
from that exact run and checks its receipt/library hashes before reuse. Only the
missing x86_64 slice should build; no receipt is rewritten and no fresh arm64 build
is authorized by missing/expired reuse data. Hosted consumer links/native arm64
fixtures and final Darwin symbol/notice review remain pending.

## Actual thin Darwin artifact review

Run [38799601657](https://github.com/ashutoshpw/zmux/actions/runs/38799601657)
at `41f1645372daca1282d0387aeb338315e2acab60` passes on ARM64 macOS 15.7.9 with
Xcode 16.4 build 16F6/SDK 15.5/Rust 1.95.0. It reused the exact retained arm64
library and built only the missing x86_64 library (89.5 seconds). The original
failed producer receipt remains unchanged and the complete library-recipe,
toolchain, patch and header identities compare exactly before reuse.

- Producer receipt SHA `a40d6c048636432a4be4b1ffd2db39e611ecf955d1ab495ec029f1dc77a89891`.
- arm64 library SHA `df2aa776d0637f13e6641ac443695c9ea4cd592416b9b1a35bf2e73be9c7e658`.
- x86_64 library SHA `3933947add88a2bbb4bc3ccec7ead6615127396536385d99069fbf46c789f941`.

Independent retained-byte review verifies both two-member generic Mach-O archives
declare library minOS 13.0; all ten C objects and four linked C/Rust executables
declare consumer minOS 14.0/SDK 15.5. Actual arm64 C ABI and Rust/C metadata owner
fixtures execute with their success markers. Intel is compile/link only. The
binary dynamic load closure is `/usr/lib/libSystem.B.dylib` only.

The 80 verbatim notices and patched headers pass integrity. The host matched all
37 runtime source hashes. Independent Mach-O symbol-table reads match the retained
`nm` logs: 563 arm64/566 Intel defined symbols. The only added symbol against the
629-symbol Linux baseline is `__isPlatformVersionAtLeast`, from already-reviewed
Zig `lib/compiler_rt/os_version_check.zig` (SHA
`8b17b5b97ce5330b68c8dcbe073686f1f4dcf633079b1caabd70fb9403b1d78a`),
whose LLVM 13 compiler-rt notice is retained. No additional runtime component or
notice gap was found in these observed symbol deltas.

**Supplementary provenance is required.** The original upload contains 137 of its
138 source-manifest files because upload-artifact excluded the hidden
`sources/repository/.github/workflows/terminal-engine-artifacts.yml`. Its exact
committed `41f1645` bytes were recovered separately and match the original expected
SHA `6fab06dc9e9709c7be6b36f78b7776c88eb3fa4e745394fe6a5ca8186eb264b7`.
The original bundle is not complete on its own; neither it nor either producer
receipt was rewritten. Future scoped uploads include hidden files. A future
no-build repack can combine immutable original inputs and this exact supplement
with a distinct repack receipt; no engine rebuild is needed for that operation.

Original upload, separate committed-source supplement and independent review are
retained outside `/tmp` in ignored
`target/terminal-engine-artifacts/darwin-38799601657/`. Review receipt
`independent-review.json` SHA
`82db21047e67fdbc370354db48343afe73b6b66b71145fa82b75a4adb1662b21`.
This clears the bounded artifact/ABI-link/native-arm64 prerequisite with the stated
supplement. It does not clear Intel/macOS 13/14 execution, production Viewport/input
conversion and publication, AppKit linkage, default-engine selection, total process
memory/time bounds or named-Mac performance gates. The original candidate verifier
continues to accept only its exact reviewed Linux candidate.
