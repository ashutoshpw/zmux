# F04 static artifact prerequisites

Original MIT tooling. This is an offline integrity and provenance prerequisite,
not an engine selector, Cargo build script, portable SDK or application package.
It downloads nothing and never compiles, extracts or launches an inspected archive.

```sh
python3 -B tools/terminal-engine-artifacts/verify.py notices
python3 -B tools/terminal-engine-artifacts/verify.py inspect \
  --library /absolute/path/libghostty-vt.a --target linux-x86_64
python3 -B tools/terminal-engine-artifacts/verify.py verify \
  --library /absolute/path/libghostty-vt.a --target linux-x86_64 \
  --producer-receipt /absolute/path/receipt.json
python3 -B -m unittest discover -s tools/terminal-engine-artifacts -p test_verify.py -v
```

`inspect` only reports the actual object format, architecture, member sizes and
SHA-256 digests. It never accepts a package. Regular GNU/BSD archives are bounded
to 64 MiB and 128 records including symbol/name tables; stored paths are strings,
never extraction destinations. Supported objects are little-endian relocatable
ELF64 or Mach-O64, x86_64 or arm64. Thin/fat archives, bitcode, unknown formats,
mixed targets, truncated members and unsupported types fail explicitly. Header
inspection does not establish object semantics, a Linux ABI or macOS minimum OS.

`verify` requires the exact reviewed candidate binary and producer-receipt hashes,
all 31 frozen base inputs plus their lock, the pinned source/header inventory,
witness, complete retained notice inventory and hash-bound independent notice review. A valid older archive or a
plausible replacement receipt cannot substitute for those bytes. The checked-in
manifest is the review trust anchor; changing it requires a new source/artifact
review. Integrity checks do not independently authenticate a newly edited manifest.
Missing/modified notices, omitted inventory entries, source drift, unsupported
targets and unresolved notice provenance reject acceptance. The `notices` command
checks retained bytes only, and explicitly reports `package_accepted: false`.

The historical accepted patched binary and original producer receipt were lost
from `/tmp`. Their recorded hashes and a verbatim later real-PTY witness are
retained as historical evidence in `candidate.json` and `evidence/`. A single
pinned reconstruction needed for the publication regression is retained in ignored
`target/terminal-engine-artifacts/20261015-publication-boundary/`; the active
manifest binds this new binary and its actual producer/focused-PTY receipts.
The rebuilt archive has two ELF64 x86_64 members and its own SHA-256; it is not
the lost historical binary. Absolute ar member paths can change archive digests.
No bit-for-bit reproducibility claim follows from equal source pins alone.

## Linked components and build-only inputs

The source pin is Ghostty `22fa134a07f5749edd9e92c6bc17d8aaf61843be` with the
unchanged original `resource.patch` then `metadata.patch`. `upstream-inputs.json`
records the verified full-archive digest and unpatched build/header member digests.
Repository hashes bind patches and C/Rust shims separately. These source references
are evidence of the minimal build graph, not a claim that every fetched package
is linked or that an older unpatched archive is the candidate.

| Component | Classification and evidence | Retained material |
| --- | --- | --- |
| Ghostty VT and original Zmux patches/shims | Runtime. `src/build/GhosttyZig.zig:initVt` roots `src/lib_vt.zig`; source pin/patch order are frozen. | Ghostty MIT; original files retain MIT declarations. |
| uucode and generated Unicode tables | Runtime code/data. `initVt` always imports uucode and `unicode_tables`; `UnicodeTables.zig:13-60` generates tables from the configured exact uucode dependency. | Exact full-archive uucode MIT, Unicode V3 and Bjoern Hoehrmann notices. |
| Zig standard/compiler/UBSan runtime | Bundled runtime. `GhosttyLibVt.zig:234-242` sets both runtime bundling flags on static libraries. Zig compiler executable itself is a build tool. | Zig MIT plus upstream-port notices described below; no claim that Zig root MIT covers all ports. |
| LLVM compiler-rt ports | Runtime-source provenance. Zig compiler_rt files name five LLVM revisions and the historical `release_80` source. | Full compiler-rt LICENSE.TXT at each exact revision; release_80 resolved to an exact commit URL. |
| musl math and Go tan source references | Conservative bundled-runtime attribution. Zig compiler_rt source explicitly references musl math files and Go math/tan.go. Origin is unversioned in most references. | Exact Zig-bundled musl COPYRIGHT; file-level notices from musl 1.1.24 and 1.2.5, exact historical log/logf references, and pinned Go release BSD/Cephes source notices. Independent coverage review accepts the observed references, including the Cephes quoted free-use terms carried by Go tan; its exact receipt is retained. |
| Wuffs, simdutf/highway, Oniguruma | Outside this minimal runtime graph: SIMD false, graphics disabled by the explicit VT feature list, `initVt` imports Wuffs only with Kitty graphics, and `GhosttyZig.zig` forces Oniguruma false. | No unrelated dependency source/license import. Enabling those features needs a new inventory. |
| translate-c/aro, platform SDK helpers and other shared build resolution | Build configuration/tool inputs; not evidence of runtime inclusion merely because dependencies are fetched. `initVt` defines the minimal VT module separately from the full-app `SharedDeps.add` graph. | Do not ship build tools with the archive under this inventory. New linked components require source-backed review and their full notices. |

uucode's Zig package `.paths` omits `licenses/`, even though its root license and
`src/utf8.zig` refer to them. The full exact commit archive was therefore verified
and used to retain both notices. Its full-tar SHA differs from Zig's filtered
package hash; both identities have different scopes.

`notices.json` records exact source URLs, full archive/member identities where
available and the retained bytes' hashes. Musl's unannotated original math files
are explicitly assigned root MIT terms by its COPYRIGHT; their inspected source
hashes/classification are in `musl-root-classification.json`. File-level math
notice blocks are retained verbatim. An unversioned port reference plus sampled releases
are not proof of the precise original source revision. The independent factual
review in `evidence/notice-coverage-review.json` checked all 80 notices, 37 Zig
source hashes, both musl release archives/file notices/root classifications, exact
historical log sources and Go BSD/Cephes comments. It found no missing notice or
copyright for the two reviewed coverage gates. The manifest binds that review to
the actual retained artifact and matching notice/source inventories. Unknown
components or unresolved coverage still reject acceptance.

No result here proves Darwin slices, universal assembly, explicit deployment
configuration, ABI compatibility, native app linkage/execution, production
Viewport/input conversion or named-Mac performance and memory workloads. Those
remain separate gates. No running cmux application is touched.

The separate [two-slice Darwin recipe](DARWIN_ARTIFACTS.md) adds manual retained
artifact production and Mach-O deployment checks. Its hosted execution is pending;
it does not change this Linux candidate or the frozen feasibility/PTY inputs.

The focused suite has ten tests on the host retaining the exact candidate. Two
acceptance tests use that real archive and actual producer receipt: scoped integrity
acceptance passes, while an altered fixture with unresolved provenance rejects the
same valid bytes. Other hosts explicitly skip those two tests when the retained
archive is absent; compiler/ar format and notice failure tests still run. A skip
never establishes candidate acceptance.
