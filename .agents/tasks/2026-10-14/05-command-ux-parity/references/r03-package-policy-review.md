# R03 dependency-package notice policy review

2026-10-15. Candidate policy for independent review before binary publication.

Exact policy SHA256: `1dba7394d65347f3ec8042b53ddb57341c6b46482f24118e10962e94931e8d79`.

## Four explicit provenance uncertainties

These published packages declare standard licenses but contain no license text at their published registry root. The standard-term supplements below are our explicit policy proposals; they are not recovered notices from these published versions. Pristine package archives and their existing source/README attributions remain included. Manifest author fields are retained as attribution evidence, without treating them as verified copyright-holder declarations.

| Package | Published license declaration | Published manifest authors | Proposed retained branch |
| --- | --- | --- | --- |
| backtrace-ext 0.2.1 | `MIT OR Apache-2.0` | empty author list | `Apache-2.0` |
| pretty-bytes 0.2.2 | `MIT` | Kohei Hasegawa | `MIT` |
| sptr 0.3.2 | `MIT OR Apache-2.0` | empty author list | `Apache-2.0` |
| interprocess 1.2.1 | `MIT OR Apache-2.0` | Kotauskas <v.toncharov@gmail.com> | `Apache-2.0` |

For backtrace-ext and sptr the published manifest supplies an empty authors list. No holder is inferred from the repository owner. For interprocess the exact author string is `Kotauskas <v.toncharov@gmail.com>`. For pretty-bytes it is `Kohei Hasegawa`; its README says “Ported from sindresorhus/pretty-bytes” and links that upstream project. The pretty-bytes source has no `.cargo_vcs_info.json`; its registry archive checksum is the immutable source boundary.

### backtrace-ext 0.2.1

- Registry archive SHA256: `537beee3be4a18fb023b570f80e3ae28003db9167a751266b259926e25539d50`.
- Declared repository: https://github.com/gankra/backtrace-ext.
- Published VCS record: `043c95350875a36be6cd755dcef21a44a52ec2cc`.
- Published `Cargo.toml` SHA256: `6c19e27c8d3f3c65ae6720d5715036faa3ab6f6187853d125e24d5ef5295d524`.
- Published `Cargo.toml.orig` SHA256: `0205a3bf2c5eee4cf95ef2b8977629e6b748035f0ddc5de200479e6ed37eaeb3`.
- Published `README.md` SHA256: `0545686a94177567dd9c957f137256b3d49eba77560f3b12a4b5c6c8d6546179`.
- Proposed decision: Published source commit 043c95350875a36be6cd755dcef21a44a52ec2cc declares MIT OR Apache-2.0 but carries no license file; select declared Apache branch with canonical standard terms, retain pristine source and manifest attribution; no upstream-version license-file or copyright-holder claim.

### pretty-bytes 0.2.2

- Registry archive SHA256: `009d6edd2c1dbf2e1c0cd48a2f7766e03498d49ada7109a01c6911815c685316`.
- Declared repository: https://github.com/banyan/rust-pretty-bytes.
- Published `Cargo.toml` SHA256: `f08f35730bc1a63b1e40f8d07ad0f7fbe575b046345421d70acb1e097b101ae4`.
- Published `Cargo.toml.orig` SHA256: `48935854675670ac1e7aeb948ce41aedd48d778f37fefe9890be41bb621dbbdb`.
- Published `README.md` SHA256: `d9616306a4efaccd43ee6294a09da0da2fa71329bd0fddb713bb12e7a1073775`.
- Proposed decision: Verified published manifest and README declare MIT without license text; retain canonical permission terms plus original source/README attribution (Kohei Hasegawa; credited port of sindresorhus/pretty-bytes); no invented copyright year.

### sptr 0.3.2

- Registry archive SHA256: `3b9b39299b249ad65f3b7e96443bad61c02ca5cd3589f46cb6d610a0fd6c0d6a`.
- Declared repository: https://github.com/Gankra/sptr.
- Published VCS record: `e59792814876b4948279ec3121921fbe827b1077`.
- Published `Cargo.toml` SHA256: `55ca3d8830461a6aac89be106ca2cfd666e63e21f832e7493d9bbb0f33fcc36e`.
- Published `Cargo.toml.orig` SHA256: `5b0a31f0a9d41e64fea52fae884f74a931cfd3800cdaf4967a2031e24d55b0a4`.
- Published `README.md` SHA256: `a98623cb764caa7b3911431d33773013d1da99c378f69b1583a24299d0b2efde`.
- Proposed decision: Published source at e59792814876b4948279ec3121921fbe827b1077 declares MIT OR Apache-2.0 but supplies no root license file; retain standard Apache-2.0 terms selecting that declared branch, full pristine package source and manifest attribution; no upstream-version license-file claim.

### interprocess 1.2.1

- Registry archive SHA256: `81f2533f3be42fffe3b5e63b71aeca416c1c3bc33e4e27be018521e76b1f38fb`.
- Declared repository: https://github.com/kotauskas/interprocess.
- Published VCS record: `4cdf61265fec855d7e0ce997fc98f1829e29c861`.
- Published `Cargo.toml` SHA256: `78ca7ee4339480bf10c02ac1e738c42e00f49a66a9b2c684230cfbc0092c8494`.
- Published `Cargo.toml.orig` SHA256: `d943558158617baef7178331126f5ac74bb9523778112151c4918e5ac70a06a6`.
- Published `README.md` SHA256: `007ae8a697e370418b31555158e186dc3b533569bd6675f8bcc0054c777fcfef`.
- Proposed decision: Published source at 4cdf61265fec855d7e0ce997fc98f1829e29c861 declares MIT OR Apache-2.0 but supplies no root license file; retain standard Apache-2.0 terms selecting that declared branch, full pristine package source and manifest attribution; no upstream-version license-file claim.

## Exact standard-term supplements

- `canonical-Apache-2.0-terms` SHA256: `074e6e32c86a4c0ef8b3ed25b721ca23aca83df277cd88106ef7177c354615ff`.
- `canonical-MIT-terms` SHA256: `5afc22050b6dfd55677fab336d50f1ac0b9173a53a4cb6bb99b9ef2525b3306c`.

The Apache supplement is the complete standard text in the exact installed Rust 1.95.0 license inventory, with its appendix and literal template placeholders preserved. It contains no applied component copyright notice. It replaces the initially considered current backtrace-ext notice, whose applied Axo attribution must not be copied to unrelated versions/packages.

The MIT supplement retains the permission, condition and disclaimer from the same Rust inventory and removes only the template copyright placeholder. It does not invent a copyright year or holder. This makes the missing upstream-version copyright notice an explicit unresolved provenance distinction; independent review must decide whether the named MIT declaration plus retained manifest/README/source attribution is adequate for promotion.

## Review decisions required

1. Accept or reject selection of the declared Apache-2.0 alternative for backtrace-ext, sptr and interprocess using complete canonical terms, preserved published source and available attribution, with missing upstream-version license notices explicitly disclosed.
2. Accept or reject the pretty-bytes MIT standard-term supplement and retained source/manifest/README attribution despite the missing original copyright notice. If unacceptable, obtain upstream notice clarification or select a different pinned dependency before promoting.
3. Review the version/checksum-scoped recovered texts for the other 27 overrides, compound license obligations, nested native notices, MPL source availability and Rust aggregate notices. No future package/version inherits this approval.
4. Verify successful exact owner receipts, rebuilt embedded-byte extraction, ABI scope and negative gates on each requested hosted platform. Local Linux execution alone establishes no Darwin package acceptance.

Only the exact independently reviewed policy SHA is accepted by the opt-in workflow. No default acquisition, native integration, minimum-OS support, universal build or reproducible-build claim follows from package promotion.

## Source-container filtering review

The current policy additionally excludes compiled files from distributed source containers. It retains original pinned URLs/archive SHA256 values and exact excluded-member paths, byte counts and hashes, while all nonexcluded source bytes are preserved. The unfiltered original archives remain in private audit caches and are not workflow evidence uploads or package payload members.

- The upstream Zellij source tar excludes exactly the original fifteen `.wasm` members. It retains 1,331 source/legal files, with twelve relative root-license symlinks materialized as exact MIT text. `SOURCES/upstream-source-provenance.json` binds the original archive and every retained/excluded member; `SOURCES/BUILD.md` gives pinned toolchain, unchanged lock, ten plugin rebuild/copy commands, original patch and native build flags.
- `wit-parser 0.221.3` excludes one prebuilt test fixture, `tests/ui/kinds-of-deps/deps/e.wasm`. All remaining package source/legal bytes are retained. That excluded binary fixture is not required by the actual normal native/plugin build; running its upstream fixture-dependent tests from this filtered source requires separately regenerating/replacing it.
- `rustix 0.37.28` excludes sixteen prebuilt fallback outline assembly archives under `src/backend/linux_raw/arch/outline/{debug,release}/`. All assembly source and regeneration/build scripts remain. Its published build.rs selects inline assembly with the executed Rust 1.95 x86_64 target and libc on Darwin, so neither accepted target uses these fallback archives. This is no support guarantee for other architectures/toolchains.

The two registry transforms are exact version/archive-checksum scoped in `registry_source_compiled_exclusions`. Their component records retain original archive URL/SHA and separate retained archive SHA, excluded-member inventory and policy basis. They use explicit `-source.tar.gz` names instead of claiming to be original checksum-valid registry `.crate` files. The other 392 registry archives, including both MPL components, remain pristine. Source availability covers complete actual native/plugin code and lock; intentionally omitted compiled fixtures/fallbacks are explicit, not silently labeled source.

Review this source-container policy with the four standard-term supplements before accepting the new exact policy hash. The prior hash is superseded and is not a promotion assertion for these changes.

## Independent candidate acceptance

The exact 41-file candidate manifest `483f36cf891b2893e734dd427b1d1e7a8aa39ba13d05c95f94bad63896d8571a`
and policy `1dba7394d65347f3ec8042b53ddb57341c6b46482f24118e10962e94931e8d79`
passed independent implementation/source/notice review. Receipt SHA256:
`4632076cc0331eb084b6ec5ecf77dc87f7413222b973f6156028638883f1cc0b`.
It verifies all 31 override VCS mappings, 21 exact texts, 395 source containers,
1,239 payload files, three exact filters, both pristine MPL components, ten rebuilt
plugins, 28 owner cases and 13 rejection controls. The four disclosed standard-term
supplements are accepted only at these exact versions/checksums.

Commit `65471de` was dispatched to both-host opt-in workflow38819411938 after isolated
`scripts/check` passed. The retained upstream license texts intentionally preserve
original trailing whitespace rather than rewriting their verified bytes.
Hosted final-SHA package execution/download review, default acquisition and native
integration remain separate gates.
