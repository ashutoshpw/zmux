# Ghostty produced artifact acceptance review

**Date:** 2026-11-12
**Scope:** Actual bytes and provenance for the fresh `ghostty-7501-v1` Darwin
producer artifact.
**Verdict:** Accept the two exact thin Darwin libraries for opt-in
`ghostty-7501-v1` linkage.

This accepts these library bytes as linkage candidates. It does not establish
application linkage, production engine selection, Intel execution, minimum-OS
runtime behavior, or physical-hardware behavior.

## Artifact identity and bytes

- GitHub repository `ashutoshpw/zmux`; workflow run `43315098805`, attempt 1,
  job `darwin-slices` completed successfully from `main` at
  `7bf0864f9155384684f8fe92be8e9c547fdd9add`.
- Artifact `13988885464`,
  `terminal-engine-darwin-thin-7bf0864f9155384684f8fe92be8e9c547fdd9add`,
  reports 9,322,902 bytes and SHA-256
  `aa232650b06d2c9184e94cfd41f38d80d91f61901726a703aed25b7ade69b855`.
  Fresh GitHub API metadata matches the downloaded metadata. The independently
  computed ZIP SHA-256 and byte count match the API values.
- All 332 ZIP files match the extracted files byte for byte. The archive has no
  unsafe paths or symlink entries, and the extracted file set has no extras.
- The extracted `receipt.json` SHA-256 is
  `e68fd9637da96fd89c710cd525f4185fc3d0718438b99c12c41a11c3f0a2cfcc`;
  its state is `complete` and source SHA is the workflow head above. Neither
  target reused a prior library.

## Accepted library bytes

| Linkage target | Archive SHA-256 | Bytes | Mach-O members and declared minimum |
|---|---|---:|---|
| `aarch64-apple-darwin` | `37e4296467733d9ee13e0005d555969ecc6c9500183ce568381fbecb177f3ad5` | 9,082,128 | `libghostty-vt-static_zcu.o` and `compiler_rt.o`, both arm64 relocatable objects, macOS 13.0 |
| `x86_64-apple-darwin` | `ce663b3259e064bd2d3de8bccaf895bae5202753b0ac8123306cde5429467464` | 9,254,272 | `libghostty-vt-static_zcu.o` and `compiler_rt.o`, both x86_64 relocatable objects, macOS 13.0 |

I parsed each archive member from the downloaded libraries and independently
matched member names, sizes, hashes, architectures, file types, and minimum OS
against the receipt. The object SDK fields are unspecified (`0.0.0`). These are
two thin archives, not a universal archive. The recorded policy is library
objects at macOS 13.0, consumers at macOS 14.0, and requested Zig targets at
macOS 14.0. The compile commands and fixture executables use SDK 15.5 and
minimum macOS 14.0. No execution on macOS 13 or 14 is claimed.

## ARM64 linked fixtures

The actual ARM64 `c-abi` executable is 1,974,920 bytes, SHA-256
`6d3454ef9f8632a1e20438df4c90512e65b3f572191bf9926c6665deeea508f1`, Mach-O
UUID `3534c854-45df-3272-b32d-c83a8fbe3b95`, minimum macOS 14.0, SDK 15.5.
Its native phase exited 0. The retained output reports
`darwin_abi_success=true pointer_bits=64 frame=72 cell=28 metadata=40`.

The actual ARM64 `metadata-fixture` executable is 2,047,728 bytes, SHA-256
`01d3689b60ebb832bb12dbd351d182a9772ab95df349013ec23c037d68fedfa8`, Mach-O
UUID `6633ebe3-974c-39d0-9f47-fa4588d3c67a`, minimum macOS 14.0, SDK 15.5.
Its Rust/C ABI link phase and native ARM64 execution phase both exited 0. The
retained output reports `metadata_success=true` and passes the summarized
metadata, abort, RIS/DECSTR, UTF-8/bidi, cwd, and notice-ring checks.

The versioned fixture inputs match the exact workflow commit: entrypoint
`metadata.rs` `9d9f64b18cc400362e684d2e8abd96eee44a5457ed5674393287475c7caf7b3b`,
`integrated.rs`
`c115496beb3e5de74f84d21c23b34455aaa575fdf9dda57fe5c168677822a140`, and
base `metadata-owner.rs`
`8abf975e8737f093062452f71b006dd5f39927cdfd3402a133026176e4c395d1`. I
reconstructed the staged owner from those exact Git objects and the pinned
fixture patch. The result `0ba0b8fd1b99358d5b5666fc6011a0048adfe1b0fe0529eb103fa243bf8c96d1`
matches the producer receipt.

That staged source retains the old first-five event assertion and full legacy
sequence including RIS metadata. It also asserts the exact ordered events for
one input feed containing prompt, BEL-terminated 7501, ST-terminated 7501, and
RIS, plus the exact metadata tuples and ST payload. The producer then compiled
and ran the corresponding ARM64 fixture binary successfully. The retained log
hashes are `158c10dd6b1f8a38a9f861a11f6ccd62d31365092edc3e01d1250d898e5fe4a4`
for `c-abi-native-arm64.log` and
`6a4933580652ca751f6d971180390322b89ddd38933146cc281b893c514ce7be` for
`metadata-native-arm64.log`; both logs match the receipt's recorded stdout with
the producer's final line newline.

## Source, recipe, pins, and notices

- `bundle/sources/manifest.json` has 150 repository inputs and exactly matches
  the receipt inventory. I checked all 150 captured byte sequences against
  both their receipt hashes and `git show
  7bf0864f9155384684f8fe92be8e9c547fdd9add:<path>`; there were no missing or
  mismatched inputs.
- The exact source candidate is
  `tools/terminal-engine-artifacts/candidate-sets/ghostty-7501-v1/source.json`,
  SHA-256
  `d19f74d9138b065014b17fb9deef9d9a2e653e6b0fd8c022eef3d1f7868d9cd8`.
  Its four source patch pins are present and match the receipt: resource
  `0ed579ca59f7e5d5f1ca695c8bc481bbfd3b1cad9832e215c9475178e6328f02`, metadata
  `a97eb547dd8d7b3c31db1bb6773dc94719f0b59357a1d0b9ad684e67280cede7`, the
  unchanged Program Status engine supplement
  `f2ea5e25d0f9015c4118ef864ab8c6c85f64e7285a5f06cad8321516995f4925`, and the
  separate owner-fixture patch
  `1aeaedaf7004dbc2af5ff618d2c3c566f12ece2515c8a29d127b3e039a181e33`.
- All 37 pinned public headers are present and match both receipt and source
  candidate inventories. All 80 notice entries have corresponding files in
  `bundle/licenses/`; every file hash matches `bundle/notices.json`. The receipt
  records 37 runtime notice sources matched.
- I recomputed the full canonical digest over every field of `library_recipe`
  using the producer's sorted compact JSON encoding plus newline. It matches
  `6c80d3798197d5c014f8962ab2a79306a938aba399343ad32df1c6b9cec99cc8`. Its
  recipe object matches the pinned Darwin recipe, and its patch inventory,
  developer/SDK information, Ghostty source identity, Zig tool identity, and
  37-header inventory are bound by that complete digest.
- I independently streamed and hashed the two actual producer downloads:
  Ghostty commit `22fa134a07f5749edd9e92c6bc17d8aaf61843be`, 39,717,617 bytes,
  SHA-256 `a07afea0e0b01cdfbf5b418c56bfd67366929dac0f9636267de5e5637351df5f`;
  and Darwin ARM64 Zig 0.16.0, 52,238,004 bytes, SHA-256
  `b23d70deaa879b5c2d486ed3316f7eaa53e84acf6fc9cc747de152450d401489`. Both
  match the receipt. The separately pinned Linux x86_64 Zig archive in
  `upstream-inputs.json` is host-specific and is not the Darwin producer
  download.
- No captured repository input or separate source supplement is missing. The
  versioned linkage manifest should use an empty `darwin.supplement` map.

## Proof limits and linkage handoff

The Intel slice and its C ABI/Rust fixture executables compiled and linked, but
the receipt explicitly says `not_run_cross_architecture`; there are no Intel
native execution phases. `intel_execution_verified`,
`minimum_os_execution_verified`, `app_linked`, `universal_assembled`, and
`engine_selected` are false. This Linux review parsed and hashed the actual
Mach-O bytes; it did not execute them. The hosted Rust/Session linkage gates,
application acceptance, and physical hardware remain separate.

There is one downstream path constraint for the parent handoff. The versioned
linkage validator checks `independent_review_path` with
`checked_file(HERE, ...)`, and `checked_file` rejects paths resolving outside
`tools/terminal-engine-artifacts`. This review is required at the requested
`.agents/tasks/2026-11-12/01-osc-7501-support/references/` path, outside that
root. Thus the current checker cannot pin this review at its mandated location
without an in-root review copy or a validator change. No such extra file or
source change was made in this review.

The exact artifact data for the parent linkage record is: source candidate SHA
`d19f74d9138b065014b17fb9deef9d9a2e653e6b0fd8c022eef3d1f7868d9cd8`, producer
receipt SHA `e68fd9637da96fd89c710cd525f4185fc3d0718438b99c12c41a11c3f0a2cfcc`,
full library recipe SHA
`6c80d3798197d5c014f8962ab2a79306a938aba399343ad32df1c6b9cec99cc8`, the two
library hashes in the table above, and download identity run `43315098805`,
artifact `13988885464`, archive SHA
`aa232650b06d2c9184e94cfd41f38d80d91f61901726a703aed25b7ade69b855`, 9,322,902
bytes. The machine-readable record is `/tmp/zmux-osc7501-ghostty-byte-review.json`.
