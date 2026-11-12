# Artifact tooling independent recheck

**Date:** 2026-11-12
**Snapshot:** shared `main`, HEAD `d573e8b87258d426ed8ccdfea20165a6a5fc2b5a`
**Scope:** `tools/terminal-engine-artifacts` and the artifact and linkage workflows only. The shared checkout was already dirty; this review changed no source, tests, workflow, or index files.

## Verdict

No artifact-tool implementation defect was confirmed. The versioned source candidate passes its source-recipe preflight, and the linkage code is wired to compile the exact Program Status targets for both Apple architectures and execute the selected ARM test once. One P3 test-fixture fidelity gap is recorded below.

This is not linkage acceptance. The repository currently has only `candidate-sets/ghostty-7501-v1/source.json`; there is no `linkage.json`, produced library, producer receipt, or independent artifact review.

## Finding

**P3, test coverage:** the full-recipe digest test does not model the complete producer receipt body. `linkage_fixture()` in `test_candidate_sets.py:128-161` constructs `library_recipe` with only `recipe` and `headers_sha256`; the negative control at `test_candidate_sets.py:202-212` changes only `recipe.targets`. The production recipe also has `ghostty`, `zig`, `patches_sha256`, and `developer` fields (`build-darwin.py:241-245`). A future validator that hashes a subset containing the two fixture fields could pass these controls while omitting a producer sibling field.

The current implementation does hash the whole parsed `library_recipe` at `candidate_sets.py:165-171`. A temporary production-shaped control with a changed `patches_sha256` member was rejected with `producer library recipe differs from reviewed recipe pin`. Keep this behavior covered by adding those sibling fields to the fixture and mutating one outside `recipe`; no source change was made in this review.

## Rechecks

- Five focused artifact tests passed: canonical header paths and Boolean schema rejection; recipe digest and header inventory binding; invalid versioned linkage contracts; exact ARM test output requirements; and linkage source hashing for the adapter and optional Session test.
- Versioned `build-darwin.py --candidate-set ghostty-7501-v1 --check-recipe` passed with 149 frozen inputs and source manifest SHA-256 `1f3577941c34498ee1da1c3325dfbfd5c4a525c29a59fb6dba01ff1277eb3ec8`. It reported `engine_built=false` and `darwin_execution_verified=false`.
- `source.json` records the ordered `resource.patch`, `metadata.patch`, and `program-status-v0.4.patch` pins. The preflight output matched their current hashes. The manifest contains 37 canonical `include/**/*.h` paths with SHA-256 pins. The build path checks those post-patch files after applying patches (`build-darwin.py:195-202`).
- The negative controls reached the intended guards: source and linkage Boolean schemas produced their respective unsupported-schema errors; missing or non-map `darwin.supplement` produced the missing-inventory error; and an unsafe supplement path produced the invalid-pin error. The recipe and header controls assert the library-recipe and header-inventory rejection messages.
- Historical recipe and linkage preflights both passed. Historical `candidate.json`, `linkage-candidates.json`, and `evidence/` are unchanged from HEAD. The defaults remain `historical`, and the prior four adapter and four Session test name sets remain in `check-darwin-linkage.py:29-45`.
- The versioned lane filters Cargo compiler artifacts by package, exact test target name, and test kind, confines executables to its fresh target directory, then inspects and hashes each Mach-O (`check-darwin-linkage.py:206-221`, `412-463`, `500-527`). It invokes the exact ARM adapter test with `--exact` and requires one passing result (`140-151`, `464-476`). The optional Session lane does the same when `--session-runtime` is enabled (`477-545`). The workflow defaults that option to true (`.github/workflows/terminal-engine-linkage.yml:13-16`).
- At this review snapshot the exact linked Session test is present and gated by `feature = "ghostty"` at `crates/session-runtime/tests/program_status.rs:83-85`; `source_hashes()` includes that file when the Session lane is enabled (`check-darwin-linkage.py:268-291`). Its runtime assertions were left to the separate runtime reviewer.

## Proof limits

- No Apple linker, two-architecture linkage build, Mach-O output, ARM test execution, hosted workflow, or accepted library was available or run in this pass. The focused tests are not a replacement for the reported full artifact suite result of 44 passed and one Apple-linker skip.
- The source-only preflight verifies the manifest and repository inputs but does not fetch the Ghostty archive or recompute post-patch header contents. The implementation handoff reports an independent recomputation of the pinned archive, ordered patches, and all 37 headers (`references/implementation-handoff.md:188-219`); this pass verified the recorded manifest structure and current source/patch pins, not that extraction again.
- No runtime behavior in the Session test, production artifact acceptance, or hosted/physical-device behavior is claimed.
