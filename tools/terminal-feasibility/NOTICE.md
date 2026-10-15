# Terminal feasibility provenance

The local drivers and runner are original Zmux code under the repository's MIT
license. The default fixtures use public upstream C APIs and independently specified protocol
bytes. The opt-in resource experiment also calls two original pinned extension APIs. No cmux GPL/BUSL implementation, test, asset or binary is used.

`pins.json` identifies every directly downloaded source/toolchain archive and its
SHA-256. Downloads stay in a temporary directory and are removed after the run.

- Ghostty commit `22fa134a07f5749edd9e92c6bc17d8aaf61843be`: MIT, copyright
  Mitchell Hashimoto and Ghostty contributors. See its [LICENSE](https://github.com/ghostty-org/ghostty/blob/22fa134a07f5749edd9e92c6bc17d8aaf61843be/LICENSE).
  The development `libghostty-vt` C API is explicitly unstable.
- Zig 0.16.0: MIT root license, copyright Zig contributors, with separately
  licensed bundled components. The platform archive checksums were checked against
  the [official download index](https://ziglang.org/download/index.json).
- Ghostty's own Zig dependency manifest pins additional packages by Zig content
  hash. The evaluated build fetched uucode (MIT), translate-c (MIT), aro (MIT and
  Unicode License V3 data), and zlib source. This is an observed dependency list,
  not a completed linked-artifact notice audit.
- The current Zmux adapter uses the repository's locked `vt100 0.16.2` MIT
  dependency and existing [third-party notices](../../THIRD_PARTY_NOTICES.md).

The workflow uploads original JSON/log evidence only. It does not distribute the
downloaded source, Zig toolchain, compiled upstream libraries or executables.
Before shipping a library in Zmux, audit the actual linked dependency, Unicode,
compiler-runtime and platform notices and preserve their licenses. The root MIT
license does not relicense every bundled component.

`resource.patch` is an original MIT modification against the unchanged Ghostty
commit above. It adds creation-time allocator propagation and exact-prefix retention;
`resource.c`, `resource.h` and `resource.rs` are original MIT integration fixtures.
The patch preserves upstream source notices. A verbatim copy of the pinned upstream
root license is retained in [licenses/Ghostty-LICENSE](licenses/Ghostty-LICENSE), and
the runner checks it against the verified archive before applying the patch. Patch
and fixture SHA-256 hashes are recorded in each opt-in receipt. This license copy
does not complete linked-component distribution notice review.


`effects.c`, `effects.h` and `effects.rs` are original MIT public-C-API fixtures.
They introduce no additional upstream patch, dependency or distributed engine
artifact. Negative controls modify only temporary copies of the original shim.

`collector.c`/`collector.h` extract the original fixed callback collector without
changing upstream code. `resource-internal.h` and `integrated.c`/`integrated.h`/
`integrated.rs` are original MIT owner/publication integration code. They reuse the
same pinned resource patch and public C handles, add no dependency and distribute
no compiled engine. Temporary negative controls modify original shim copies only.
