# Ghostty metadata fixture repair

**State:** local source repair complete; independent review and fresh native producer remain pending.

## Failure and cause

The downloaded producer artifact was workflow run `43300002338`, artifact `13980787888`, named `terminal-engine-darwin-thin-3b385182e43e66bde01100640b9c6cf213eac048`. Its archive SHA-256 was `e282d0a8b11a9292ad95dc73e369cc8c17206ce2016be4fd09534842826f264f`. The receipt recorded `metadata-native-arm64` exit 101. The retained log panicked at `metadata-owner.rs:433` after `MetaState::apply` returned `Err(())` and the fixture unwrapped it. The captured source candidate was `1f3577941c34498ee1da1c3325dfbfd5c4a525c29a59fb6dba01ff1277eb3ec8`; it had only the historical owner and the three engine patches, with no fixture patch chain or fixture receipt.

The Program Status supplement reports OSC 133 prompt and RIS callbacks and OSC 7501 raw metadata. The immutable legacy owner accepts only its existing metadata sources and rejects those added source/subcommand pairs, so the native fixture fails before it can finish its old assertion sequence. The root cause is this mismatch between the supplement callbacks and the older fixture owner. The failure does not show a C ABI compile failure: the producer had already reached and failed the ARM native fixture phase.

## Chosen adaptation

Keep the historical `metadata-owner.rs`, the three engine patches, and their pins unchanged. Add `tools/terminal-feasibility/program-status-v0.4-owner-fixture.patch` as a separate one-entry ordered `fixture_patches` chain in `candidate-sets/ghostty-7501-v1/source.json`. The chain targets only the historical metadata fixture owner and pins its exact baseline hash. The existing engine patch order remains resource, metadata, Program Status supplement.

The fixture patch adds explicit acceptance for prompt `(source 133, subcommand 1)` and RIS `(133, 2)` only with empty data and no terminator. It accepts raw Program Status `(7501, 0)` only with BEL or ST and a nonempty payload, leaving parsing to the real Program Status owner. The fixture asserts prompt/RIS callback tuples, the RIS event sequence, raw 7501 payload bytes and terminator, and that these callbacks leave title, cwd, notices, and notice sequence unchanged. It retains the existing owner assertions and adds rejection controls for unsupported sources/subcommands, malformed 7501 callbacks, and unknown events.

The producer stages `metadata.rs`, `integrated.rs`, and `metadata-owner.rs` together in a temporary sibling directory, preserving the relative `include!` paths, then applies only the fixture patch chain. Its receipt records the original entrypoint/integrated/owner source digests, pinned base owner digest, ordered patch identities, and patched-owner digest. The top-level captured source inventory retains the repository source bytes and manifest pins. Linkage acceptance requires this fixture receipt, checks its source and patch chain against the versioned manifest, and requires the captured entrypoint/integrated/owner inputs to match the historical candidate pins as well as the receipt record.

The source manifest stays at shared document schema version 1 because `verify.document()` accepts schema 1 only. `fixture_patches` is an explicitly required, validated versioned-source contract; no generic schema change or edit to `verify.py` is needed.

## Exact source identities

| Input or output | SHA-256 |
| --- | --- |
| Historical `candidate.json` | `038aa8eb5dd381c353733550e95579221e1ea6967ae03a860c486436eb690c12` |
| Historical `linkage-candidates.json` | `595b5db8b04a9c108f614008df7364a78648e843c4d1c3e28350119bd5ff5d6d` |
| Historical Darwin recipe | `f57162b68a7866935bbd7662629b7508a4fbd6102134a5cd10efa21010be914d` |
| Historical `metadata-owner.rs` base | `8abf975e8737f093062452f71b006dd5f39927cdfd3402a133026176e4c395d1` |
| Historical metadata engine patch | `a97eb547dd8d7b3c31db1bb6773dc94719f0b59357a1d0b9ad684e67280cede7` |
| Unchanged Program Status engine supplement | `f2ea5e25d0f9015c4118ef864ab8c6c85f64e7285a5f06cad8321516995f4925` |
| New owner fixture patch | `fe4581f033ae8832be3a4eb1502d6680fa87e22d27308527a8ca16fc106551dd` |
| New `ghostty-7501-v1/source.json` | `fc65a45623707b4a9b3301e69698bcb6bf4a952e7f594f301ffa8b9e06734d04` |
| Patched staged owner output | `de0da8f9b9d4ea800234104cccc5773ac29e04709e0f2fe65a10377ebab7c601` |

The historical fixture source hashes remain `metadata.rs` `9d9f64b18cc400362e684d2e8abd96eee44a5457ed5674393287475c7caf7b3b`, `integrated.rs` `c115496beb3e5de74f84d21c23b34455aaa575fdf9dda57fe5c168677822a140`, and `metadata-owner.rs` as pinned above. The historical candidate, linkage candidate, recipe, owner, metadata patch, and Program Status supplement have no working-tree diff.

## Validation and remaining gate

- `python3 -m unittest discover -s tools/terminal-engine-artifacts -p 'test_*.py'`: 47 total: 46 passed, 1 skipped because the actual Apple linker is unavailable. The tests include a positive real-source staging/application case plus candidate and linkage corruption controls for patch order, base drift, missing/changed patch pins, absent fixture receipt, wrong fixture inputs, and malformed output digest.
- `build-darwin.py --check-recipe` passed for historical (148 frozen inputs) and `ghostty-7501-v1` (150 frozen inputs; source manifest SHA above). Both report `engine_built=false` and `darwin_execution_verified=false`.
- Applied the pinned patch through the producer staging helper in a temporary sibling-source directory and compiled the patched fixture with host `rustc 1.95.0` using `--edition=2024 --crate-type=lib --emit=metadata`; compilation passed and emitted 57,340 bytes of metadata.
- `git diff --check` passed for the owned files. `scripts/check` was not rerun; the parent owns aggregate validation.

There is no accepted `ghostty-7501-v1/linkage.json` or linked versioned library in the checkout. The metadata-only Rust compile does not resolve/link the C ABI and does not execute callbacks. The required next proof is a fresh macOS ARM64 producer run with this exact source manifest, a successful linked `metadata-native-arm64` fixture and receipt, followed by independent artifact/linkage acceptance for the exact produced libraries. The previously downloaded failed artifact remains a failed historical run, not acceptance evidence for this repair.

## Parent fixture correction after independent review

The first source review found missing mixed-callback and positive ST coverage.
Parent inspection also found stale assertions after the legacy feed was split:
the feed without RIS still expected reset event 3, and the later separate feed
replaced the snapshot whose DSR reply the old assertion checked.

The final patch restores the original combined legacy input including RIS,
retains its first-five-event assertion, and checks the full six events including
the new RIS metadata callback. One new mixed prompt/BEL/ST/RIS input checks
exact event order, metadata association, both payload byte ranges and legacy
state independence. The historical owner and three engine patches stay fixed.

Final patch: `1aeaedaf7004dbc2af5ff618d2c3c566f12ece2515c8a29d127b3e039a181e33`.
Final source manifest: `d19f74d9138b065014b17fb9deef9d9a2e653e6b0fd8c022eef3d1f7868d9cd8`.
Final staged owner: `0ba0b8fd1b99358d5b5666fc6011a0048adfe1b0fe0529eb103fa243bf8c96d1`.

Parent reran the policy suite: **47 total, 46 passed, 1 Apple-linker skip**.
Versioned preflight and staged Rust metadata compile passed. Metadata compilation
does not link or execute the C callbacks; fresh native production remains a gate.
The earlier hash table records the first reviewed adaptation, superseded by the
final hashes above. Focused final review and aggregate validation are pending.
