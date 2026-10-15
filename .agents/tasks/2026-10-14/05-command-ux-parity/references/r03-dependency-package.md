# R03 retained dependency package audit

2026-10-15. Source-backed proposal for a future explicit native consumer. No binary
upload, product acquisition switch, native launch selection or complete notice
acceptance is implemented by this audit.

## Established execution scope

Patched-source owner run 38811822523 at
0ffc1105da85b51568172cb7946665d67923ff1e passed the 27 existing production owner
checks on Linux X64/glibc 2.39 and macOS 15.7.9 ARM64. The verified combined receipt
is `target/agent-validation/r03-hosted-38811822523/verified-receipt.json`, SHA256
`e561705c13f20fada3eb5b6541807993e242942db57643629861e9d8f1eed7a4`.
No compiled binary was uploaded. The native consumer therefore cannot acquire a
retained, independently hashable executable from that evidence artifact.

This is not Intel Darwin, universal macOS, a minimum-OS guarantee, native semantic
attachment or an automatic acquisition decision. Darwin's two builds share binary
hash `a7a3355493177ac2094ffd569d0339e566bc234f96d03363bb5785619fc6f9f5`;
Linux's hashes differ between builds with the same pinned source/Rust version.
Packages must retain the same binary tested in their own job, not rebuild it after
acceptance or infer byte equivalence from the source pin.

## Source and asset findings

The immutable MIT Zellij source is commit
23bb542548d517f7f20a7fdd14a943f1a610df37. Its archive hash, upstream Cargo.lock,
original EINTR patch and build flags are already bound by the producer receipt.

At that commit, `zellij-utils/src/consts.rs:100` embeds release plugin bytes from
`assets/plugins`; its ASSET_MAP at line 123 selects exactly ten production plugins:
compact-bar, status-bar, tab-bar, strider, session-manager, configuration,
plugin-manager, about, share and multiple-select. The archive contains 15 WASM
files: those ten, one test fixture in the current plugin directory, and four
legacy files elsewhere. The current 15-file inventory is an archive inventory,
not an assertion that all 15 ship in the executable.

All 11 current-directory WASM binaries have no custom sections, including no
producer or dependency-lock metadata. Their archive hashes prove the exact
upstream bytes; the source lock alone does not prove which transitive versions
were compiled into those stripped binaries. The root MIT license is insufficient
to invent that missing compiled-component provenance.

Recommended closure: build the ten ASSET_MAP plugins from this same pinned source
and unmodified lock using Rust 1.95.0 and `wasm32-wasip1`, then copy only those ten
results to the release include paths before compiling the native binary. Upstream
`default-plugins/*/.cargo/config.toml` already selects that target, and
`xtask/src/build.rs:130` documents the same release copy boundary. Direct locked
Cargo builds can use the already checked-in generated protobuf assets, avoiding
unrelated xtask regeneration, publishing and optimizer installation. Record every
original/rebuilt plugin hash and the actual compiler units. A changed plugin
bundle produces a new native artifact and needs its own owner acceptance; the
currently accepted hashes do not transfer to it.

## Distribution obligations to retain

The source audit used the same locked no-default-features/vendored_curl native
normal/build graphs for Linux and Darwin, plus a wasm32-wasip1 graph rooted at the
ten selected plugin packages. Logs and phase receipts are under
`target/agent-validation/r03-package-audit-20261015/`. These are resolved source
graphs, not proof of the old prebuilt plugins' dependencies or final linked units.

- `colored 2.0.0` and `option-ext 0.2.0` are MPL-2.0 in the native and proposed
  plugin source graphs. Retain their license/notices and provide the corresponding
  covered source with an explicit source-availability notice. Pristine registry
  `.crate` archives are available and match Cargo.lock checksums: respectively
  `b3616f750b84d8f0de8a58bda93e08e2a81ad3f523089b05f1dffecab48c6cbd`
  and `04744f49eae99ab78e0d5c0b603ab218f515ea8cfe5a456d7629ad883a3b6e7d`.
- Compound expressions need their additional components: `encoding_rs 0.8.34`
  declares `(Apache-2.0 OR MIT) AND BSD-3-Clause`; `wezterm-bidi 0.2.3` declares
  `MIT AND Unicode-DFS-2016`. Do not reduce every dependency to a root MIT choice.
- Wasmtime/Cranelift use Apache-2.0 with LLVM exception. Preserve their actual
  license/NOTICE material and the exception rather than substituting plain Apache.
- Native vendored libraries have distinct texts below wrapper-crate roots:
  `curl-sys/curl/COPYING`, `openssl-src/openssl/LICENSE.txt`,
  `libnghttp2-sys/nghttp2/COPYING` and zlib's header notice are concrete examples.
  Capture actual native linkage to distinguish system libraries from embedded
  vendored code. The local Linux artifact links system zlib, libc/libm and libgcc;
  no equivalent linkage receipt was collected for the hosted binaries yet.
- Rust's statically linked standard library carries its own third-party inventory.
  The exact installed 1.95.0 toolchain includes `share/doc/rust/COPYRIGHT-library.html`
  and the referenced `licenses/` directory. Retain these with toolchain hashes,
  including the wasm target's standard-library/WASI contribution scope.
- Upstream local plugin manifests for session-manager and plugin-manager omit a
  package license field. Record their reviewed coverage by the verified repository
  MIT root; do not silently treat arbitrary future missing registry licenses alike.

The existing Zmux `scripts/license-inventory` explicitly limits itself to available
top-level package license files. It is useful for conventions, but is not a
complete standalone Zellij binary/embedded-asset distribution generator. The
529-package unfiltered workspace metadata also contains dev/profiling packages
outside the native normal/build graph, such as dual GPL/BSD ittapi. Build/license
scope must not be inferred from every metadata package being present. Prefer
package IDs from the final native and plugin compiler-artifact records, retaining
normal/build support dependencies conservatively with their role marked.

## Proposed narrow package slice

Keep existing official acquisition pins and ordinary terminals unchanged. Extend
the manual patched-owner workflow with an explicit opt-in package input, false by
default, and shared producer/package helpers:

1. Install only the pinned Rust wasm32-wasip1 target when packaging is requested.
   The producer rebuilds the ten selected plugins, records old/new asset hashes,
   and captures final native/plugin compiler-artifact records with the unchanged
   upstream lock. Existing non-package feasibility mode remains available.
2. Generate a fail-closed component and notice manifest from actual build units,
   registry archive checksums, reviewed local-root coverage, nested native notice
   mappings and the actual Rust standard-library notice files. Include exact
   corresponding MPL source archives and source-availability text. Missing text,
   unreviewed compound licensing or unknown embedded bytes prevents packaging.
3. Record Rust target/toolchain, compiler and linker versions, OS/architecture,
   effective flags/config hashes, SDK and deployment target on Darwin, Mach-O load
   commands/dylibs or ELF interpreter/GLIBC requirements. Claim only executed host
   and recorded ABI scope; no universal or backward-OS guarantee is inferred.
4. Run the existing bounded owner helper against that exact retained executable.
   Validate all ten generated WASM modules with the pinned runtime and retain the
   default-plugin execution exercised by the real owner journey. Any unexercised
   optional plugin UI behavior stays explicit rather than becoming a parity claim.
5. Only after notices and owner gates pass, create an architecture-specific package
   containing `bin/zellij`, manifest, complete retained notices, corresponding
   sources and source/patch/build/owner provenance. Bind every package file and
   the final archive by SHA256. Do not rebuild or modify the executable after its
   owner gate. Preserve the distinction between an ad-hoc signature and app signing.
6. Upload that package in a separate conditional artifact step. Existing evidence
   still uploads on failures; failed/incomplete packages never upload executable
   bytes. A future native consumer must explicitly select and verify the package;
   package creation does not change product defaults or authorize an acquisition
   swap. Native application embedding, signing and semantic attachment remain the
   consumer's own gates.

Implementation candidates are a bounded extension to `produce-patched.py`, a
separate `package-patched.py` helper and a reviewed notice/component policy file.
The workflow shares those owners instead of duplicating build/test logic. Before
binary publication, the rebuilt-plugin producer, manifest/notice resolution,
negative gates and both actual host journeys need independent source review and
fresh executed receipts.

## Implementation candidate and review boundary

The explicit opt-in producer/package/workflow candidate is implemented locally.
The initial Linux prepare-only run passed exact embedded-byte extraction and
retained 410 actual native/plugin compiler components, including 394 pristine
registry source archives; it was marked unreviewed and was not uploaded. A fresh
chain is being executed after tightening metadata/source binding and owner
behavior admission. The current owner suite has 28 passing checks in six suites,
including every named baseline behavior; that count is a report, not parity proof.

See [the exact policy review brief](r03-package-policy-review.md) before promotion.
It preserves the four explicit metadata-only standard-term supplements and the
independent policy decisions required. Existing default workflow execution still
uploads no executable. Opt-in publication requires the exact reviewed policy SHA,
all owner/source/plugin/notice gates and the separate successful-package artifact
step. Hosted package execution on both requested architectures remains pending.

## Frozen local validation receipt

The tightened source/plugin/owner/package path passed locally on Linux x86_64.

- Producer receipt: `target/agent-validation/zellij-package-producer-v2-20261015/receipt.json`; real EINTR red/green, non-EINTR fatal controls, ten rebuilt WASM plugins and pinned Wasmtime compile checks passed.
- Owner receipt: `target/agent-validation/zellij-package-owner-v2-20261015/receipt.json`; `28` checks passed across suites `[2, 14, 3, 4, 3, 2]`, including every baseline behavior and mandatory real input/resize/stop-resume/TERM/HUP/reattach/termios journey field. Before/after source inventory covers `139` exact Rust/manifest/config inputs. Reported source base SHA: `c9404cfb2d66d0a1d327d3f4e1edae061c495c2a`; the local source inventory, rather than a clean-checkout claim, binds concurrent working source.
- Admission controls: `target/agent-validation/zellij-package-negative-v2-20261015/receipt.json`; `9` intended negative cases rejected after a positive exact-chain control. A fixture initially looked for logs below the receipt filename; that harness-only path error was fixed and a fresh run passed.
- Prepare-only receipt: `target/agent-validation/zellij-package-review-v3-20261015/receipt.json`; `promotion_ready=false`, exact binary SHA256 `d2b9ea3857ee64e9f51a58084d7f8f792b9ac38bf915ef6ad5cf61a4d7d5061c`.
- Review archive SHA256: `d8090f768ec60bc9aaaaa89c28d38aaf0380c6169dfc99ee1d24f49de5e64b2b`. It contains 410 actual native/plugin compiler components and 394 pristine registry archives; no validator-only dev closure.
- Independent archive verification: `target/agent-validation/zellij-package-review-v3-20261015/verified-archive.json`; every one of 1,236 regular members matched manifest size/hash, component notice/source references resolved, and both MPL component source archives were retained. Payload total: 113,022,597 bytes.
- Exact notice-policy SHA256: `e2945ed1ff4646c2cde4c90b7f138df08d433c9e7dcfdbcd4c4303e356cf68be`. All four metadata-only supplements remain explicitly disclosed and unapproved for publication.
- Linux recorded GLIBC symbol maximum: `2.34`; interpreter `/lib64/ld-linux-x86-64.so.2`. This is observed ELF metadata, not a backward-host support claim.
- Python AST syntax and workflow YAML parsing passed. Repository-wide `scripts/check` is coordinated by the parent task owner; no standalone native build was needed for this portable tooling slice.
- No archive/executable bytes were uploaded, no default pin/acquisition/native selection changed, and no stage/commit/push/workflow dispatch was performed by this task owner.

Independent source/policy review and both-host hosted package execution remain before promotion. The earlier local archive from the initial candidate is superseded for review by this tightened V3 archive; neither is approved for publication.

## Refrozen source-filtered candidate

The V3 source-container issue is repaired; V4 supersedes V3 for review. No original
upstream prebuilt WASM bytes are distributed inside the source archive. The private
pinned original archive remains available for audit, with its URL/SHA and all fifteen
excluded member paths, sizes and hashes retained in
`SOURCES/upstream-source-provenance.json`. The filtered source tar preserves every
other original source member, materializing twelve relative MIT license links as
exact regular-file license bytes. It includes 1,331 retained source/legal files,
unchanged Cargo.lock, original patch and exact pinned native/plugin build instructions.

A full registry source-container scan additionally found one wit-parser WASM test
fixture and sixteen unused rustix fallback static archives. Those two exact
version/checksum-scoped source transforms preserve all other source/legal/assembly
and regeneration files; their component records bind original URLs/archive hashes,
filtered archive hashes and every excluded member. The two filtered bundles use
`-source.tar.gz` names. The other 392 registry source archives, including both MPL
components, remain pristine. Empty vcpkg DLL/static-archive-named test placeholders
contain no compiled bytes and are preserved. No ELF/PE/Mach-O source payload was found.

- Fresh actual owner receipt: `target/agent-validation/zellij-package-owner-v3-20261015/receipt.json`;
  all 28 checks in six suites `[2, 14, 3, 4, 3, 2]`, every baseline behavior and full
  real semantic journey passed, with 141 exact source/manifest/config inputs retained.
- Adversarial receipt: `target/agent-validation/zellij-package-negative-v4-20261015/receipt.json`;
  all 13 intended rejections passed, including disguised old WASM inside a source tar,
  lost Cargo.lock, altered excluded-member provenance and unreviewed registry WASM.
- Prepare-only receipt: `target/agent-validation/zellij-package-review-v4-20261015/receipt.json`;
  `promotion_ready=false`; exact accepted binary remains
  `d2b9ea3857ee64e9f51a58084d7f8f792b9ac38bf915ef6ad5cf61a4d7d5061c`.
- Current unreviewed archive SHA256:
  `637799f69b5a5945d17d441670c2ba2cbfba25f8f382a48235a54f546545a932`.
- Current exact policy SHA256:
  `1dba7394d65347f3ec8042b53ddb57341c6b46482f24118e10962e94931e8d79`.
- Independent `verified-archive.json` checked all 1,239 outer members and hashes,
  plus all 395 nested source containers for WASM/native/static-archive magic.
  Payload bytes: 108,308,615. `verified-source-transforms.json` independently compared
  every retained/excluded member with the private pinned original source archives:
  upstream 1,331 retained/15 excluded, wit-parser 466/1, rustix 256/16.
- Python syntax, workflow YAML parse and scoped diff whitespace checks passed.
  Repository-wide validation remains coordinated by the parent task owner.

The original unfiltered archive is omitted from both package payload and workflow
evidence upload paths. Defaults/acquisition/native selection remain unchanged.
Independent implementation/policy review and both-host hosted execution still remain;
no executable publication, staging, commit, push or dispatch was performed here.


## Hosted metadata capture failure and reviewed repair

Hosted package run `38819411938`, source SHA
`65471decedaf8e607da66f8499334015e76f7c53`, completed both Linux x86_64 and
Darwin arm64 producer/actual-owner steps successfully. Both then failed the
package admission gate because fresh Cargo metadata downloads wrote progress to
stderr, which the existing combined diagnostic capture placed before JSON.
Neither host uploaded a successful package. Logs are retained under
`target/agent-validation/r03-package-hosted-38819411938/`.

The package-local `capture-stdout.py` repair writes Cargo stdout to a fresh,
exclusive, bounded sink while keeping stderr in the existing bounded parent log.
It remains inside the unchanged retained process-group owner and preserves
nonzero command failures. The producer parses the metadata before passing; the
actual-owner and package provenance bind the helper hash. This changes no notice
policy, shared process owner, default acquisition or native selection.

- Actual fresh-download red/green: `target/agent-validation/r03-cargo-metadata-fresh-v2-20261015/receipt.json`; the same Cargo/source/lock command with an absent private-cache crate reproduces the legacy JSONDecodeError, then captures a valid 529-package graph with real download diagnostics retained separately. Global caches and Cargo.lock were not modified.
- Seven stream/lifecycle/overwrite/failure tests passed; eleven unchanged shared-owner cases also passed. Evidence and the six-file freeze are in `target/agent-validation/r03-package-hosted-38819411938/`. Freeze SHA256: `c52b4d7bc4c410a0ed3d824cd359d829d7c196c6908601e995f0557f8e51b35c`.
- Independent capture review approved with no blockers: `/tmp/zmux-r03-package-capture-independent-review-20261015.json`, SHA256 `a4db848b0d18a942d2a38e481c757245ac9b61ccfdbda839ea3f7123ecdb3db6`.
- Fresh complete producer: `target/agent-validation/zellij-package-producer-v3-20261015/receipt.json`; ten rebuilt plugins, Wasmtime validation, actual EINTR red/green, non-EINTR controls and valid metadata passed.
- Fresh actual owner: `target/agent-validation/zellij-package-owner-capture-v1-20261015/receipt.json`; current source executes 29 checks in suites `[2, 15, 3, 4, 3, 2]`, binding 145 source inputs and the full actual owner journey. The count increased with concurrent owner work; the semantic contract remains the admission gate.
- Fresh adversarial controls: `target/agent-validation/zellij-package-negative-capture-v1-20261015/receipt.json`; all thirteen intended rejections passed.
- Fresh prepare-only package: `target/agent-validation/zellij-package-review-capture-v1-20261015/receipt.json`; `promotion_ready=false`, binary SHA256 `472558df21510d4a29d47cc651d01ae1418f71812b616216542e6036c5406803`, archive SHA256 `0b0476476519f383d1707467524ac71a2de9aec8652559af47cb4abe1c6d788b`.
- Its independent `verified-archive.json` checked all 1,241 outer members against the manifest, capture source provenance and all 395 nested source containers (25,745 members) for compiled payloads. No packaged executable was executed during this independent scan.
- Shared-owner SHA256 remains `8c2f70b650b72c58764081ac764bd5fea04a5a015ffc2407db73a2b29433ea2f`; independently reviewed notice-policy SHA256 remains `1dba7394d65347f3ec8042b53ddb57341c6b46482f24118e10962e94931e8d79`.

A committed current-source replacement run on both hosts and independent downloaded
package verification remain. Parent subsequently authorized this task owner to
run required `scripts/check`, then stage only the six frozen repair files plus
this owned audit note, commit, push and dispatch the unchanged-policy replacement
workflow. The required check runs from a private source snapshot of `HEAD` plus
only this repair, reusing the existing owned Cargo cache; its receipt is
`target/agent-validation/r03-package-hosted-38819411938/capture-repair-check-receipt.json`.
Concurrent runtime edits and parent-owned task records are excluded from this
source snapshot and commit. Hosted run and final artifact receipts are retained
separately after dispatch.
