# F04 explicit production adapter linkage seam

2026-10-15. Original MIT production bridge/adapter slice; no default engine switch.

- `crates/ghostty-vt` isolates unsafe FFI, retains a non-Send worker-bound owner,
  reuses the frozen reviewed C Resource/Integrated/collector/metadata machinery and
  performs full-cell export before the existing resource/export seal. The original
  experiment inputs and retained libraries are unchanged. Constructor geometry,
  history and explicit mapping limits arrive before terminal creation.
- `terminal-adapter::Engine` accepts explicit backend selection and stages the
  existing complete owned `Viewport`, title/cwd/notices and immutable bounded reply
  bytes under one checked generation. Exact commit retires C metadata and moves
  the candidate without allocation. Abort is permanent and retains last publication;
  unsupported public style/palette/link data fails visibly. Default existing
  `Terminal`, runtime worker and app engine selection are unchanged.
- `accept-linkage.py` gates exact Linux reconstruction and both retained Darwin
  thin library hashes. Darwin requires original successful receipt, independent
  review, all source/header identities and the separately retained exact workflow
  supplement. No producer receipt is rewritten. Build.rs requires an explicit
  retained root, compiles only the small C shim, and never downloads, discovers a
  fallback library or rebuilds Ghostty.

Local evidence: default adapter tests pass10; actual retained-library staged adapter
suite passes4, including cells/Unicode, exact semicolon metadata, causal replies,
zero-reply commit, wrong-generation/mutation refusal, failure/abort last-publication
retention, exact1,000 history, scroll and alternate transitions. Strict linked
Clippy passes. Isolated0101763 plus this linkage slice passes full scripts/check283
with formatting, workspace Clippy, notices and C ABI; independent linked4 rerun
passes. Both thin Darwin packages pass input acceptance; altered Linux
library bytes and unsupported targets refuse. No Darwin compilation/execution of
this new shim has occurred. The existing Darwin fixture receipt remains separate.

Worker integration is deliberately pending the shared shutdown slice. Required
contract: preserve previous partial and admitted input before replies in the sole
physical PTY writer; no PTY reads, terminal mutation or new input dequeue/admission
while staged; suppress POLLIN while blocked but retain cancellation/deadline/exit
wakes; zero-reply stages still commit. On cancel/deadline/partial-write error abort
once, fail pending receipts once with partial-write uncertainty, retain published
metadata/notices, and never replay/fall back. Engine resize stages first, then kernel
PTY resize; kernel failure aborts visibly with the old published view and no
rollback-atomicity claim. Root/storage must agree the close/EOF/deadline integration
before worker edits.

Typed key/paste/wheel encoding, native presentation, selection/IME, workload
performance, production mapping/host budgets and default selection remain gates.
The explicit fixture mapping caps are not product defaults or total-RSS limits.
See the bridge README and existing resource/metadata contracts for bounded storage
and unexecuted branch exclusions. This is concrete production linkage code, not a
claim of full terminal parity or completed application migration.

## Retained Darwin linkage workflow

The manual `Terminal engine retained Darwin linkage` lane consumes successful
run38799601657 artifact12004400055 (ZIP SHA256
4412eae9e095726b7a0fad080f87358c8b633f45c481376aba2f4fd73eecd600), whose exact
original engine bytes and producer receipt remain unchanged. The exact independent
review is retained as evidence. The missing hidden workflow source is recovered
separately from commit41f1645 and checked against its original hash. Metadata,
compressed/expanded size, paths, file types, hashes and both target identities fail
closed; there is no newest-artifact substitution or rebuild fallback.

The new driver builds only the production shim and staged adapter test with pinned
Rust1.95.0, Xcode16.4/SDK15.5 and consumer14.0. Cargo's exact package artifact messages
select the fresh object/executable, both Apple targets receive raw Mach-O and
runtime-load inspection, and only the ARM executable runs all4 behavioral tests.
Intel remains compile/link/inspection only. Original input provenance and new
object/executable/log receipts are retained separately in the linkage artifact.
The workflow never invokes Zig or the engine producer. Native linkage remains
unexecuted until the committed lane is dispatched and its actual bytes reviewed;
no worker/default/native-app/input/performance acceptance follows from this lane.

The next production worker slice is tracked in [F04 Session engine integration](f04-session-engine.md).
