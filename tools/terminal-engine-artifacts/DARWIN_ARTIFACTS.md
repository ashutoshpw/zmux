# Two thin Darwin artifact slices

This manual prerequisite produces retained arm64 and x86_64 archives from the
reviewed minimal source graph. It leaves the feasibility/PTY source locks and
application engine unchanged. Hosted execution of this new recipe is pending.

```sh
# Portable source/notice/recipe validation; no download or build:
python3 -B tools/terminal-engine-artifacts/build-darwin.py --check-recipe
python3 -B -m unittest discover -s tools/terminal-engine-artifacts -p 'test_*.py' -v

# On a macOS arm64 host with Xcode 16.4/SDK 15.5 and both Rust target libraries:
python3 -B tools/terminal-engine-artifacts/build-darwin.py --output /absolute/new-output

# After a later compile/fixture failure, retain the old output and use a NEW path:
python3 -B tools/terminal-engine-artifacts/build-darwin.py \
  --reuse-artifacts /absolute/old-output --output /absolute/new-output
```

The manual `Terminal engine Darwin artifacts` workflow has one macos-15 arm64 job.
It selects `/Applications/Xcode_16.4.app/Contents/Developer`, requires Xcode 16.4 and
SDK 15.5, and records Xcode build identity, SDK settings hash and compiler versions.
`darwin-recipe.json` fixes Zig 0.16.0, Rust 1.95.0, ReleaseSafe, two build jobs,
SIMD false and the existing explicit VT feature list. Exactly two thin targets
are built: `aarch64-macos.14.0` and `x86_64-macos.14.0`. There is no Linux engine
rebuild, feature matrix, Rosetta installation or universal assembly.

The driver uses the pinned upstream archives, original resource then metadata
patches and all 32 reviewed source inputs. A nested Git root and checked patched
headers prevent silent patch skipping beneath an ignored parent checkout. The
actual Darwin toolchain's 37 referenced runtime source hashes must match the
reviewed notice inventory. Both thin archives retain full required notices and
the factual coverage review. New defined/undefined symbol observations and their
delta from the actual 629-symbol Linux baseline are retained for Darwin review;
equal source hashes alone do not establish every platform artifact's acceptance.

`darwin_platform.py` shares the bounded regular GNU/BSD archive reader with the
existing verifier. Every payload must be a relocatable Mach-O64 object of the
requested generic CPU subtype (specialized x86_64h/arm64e is rejected). Bounded load-command parsing checks macOS platform and an
exact declared minimum of 14.0 for every archive member and C shim. It rejects
missing, duplicate, truncated, iOS/other-platform or mismatched declarations.
An object's SDK field can be zero (unspecified) or 15.5; zero is recorded explicitly
and does not prove the SDK. The actual build SDK is checked separately. Tests use
real Clang-produced Mach-O objects and archives, including older macOS, alternate
SDK and iOS objects; corrupted bytes are used only for negative cases.

Both targets compile and link an independent C header/size/offset consumer and
the existing Rust metadata owner against the actual archive. Required public and
original extension symbols must be present. Only arm64 C/Rust fixtures execute,
with the existing bounded subprocess helper and explicit success markers. The
x86_64 target is recorded as `not_run_cross_architecture`. No result proves Intel
runtime behavior, execution on macOS 14, AppKit integration, production Viewport/
input behavior, performance, memory budgets or a default-engine decision.

Output contains `receipt.json`, bounded per-phase logs and `bundle/` with both
thin libraries, patched headers, full notices/review, C objects and C/Rust fixture
binaries. `bundle/sources/repository/` retains the exact original tooling, source
pins, both raw patches and C/Rust fixture inputs, with their full hash/commit map.
An offline reviewer can inspect patch contents even if the original checkout is
gone. Full upstream/toolchain archives stay URL/hash referenced; this is not a
redistribution of the whole SDK or a promise of an offline compiler environment.
A library's path/hash is recorded immediately after its build, before
later inspection or fixtures. `if: always()` uploads those files on failure as
well as success; work/download caches are excluded. Bundle acceptance caps at
256 MiB, each archive at 64 MiB/128 records, builds at 600 seconds and native
fixtures at 10 seconds. Downloads and process/log containment reuse the frozen
accepted runner without editing it.

Explicit reuse checks the prior library recipe, selected toolchain/SDK/header
identities, retained archive hash and current Mach-O/deployment invariants. It
refuses mismatches rather than silently rebuilding a supplied bad slice. Missing
previous slices are built once; C objects/fixtures are cheaply rebuilt to test a
repair. Reuse records the exact prior receipt digest. Receipts are provenance
records, not cryptographic authentication of an arbitrary edited bundle.

The recipe preserves actual binary and input hashes and a repeatable configuration.
Archive member paths/debug data can still affect byte identity; a second independent
bit-for-bit rebuild is not claimed. The existing `verify` candidate remains the
reviewed Linux archive. It continues to reject macOS targets until actual hosted
bytes and their platform inventory are reviewed and separate manifests are added.
No completed build automatically updates candidate selection or links an app.
