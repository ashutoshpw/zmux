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
