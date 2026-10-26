# F04 exact history and allocation options

2026-10-15. Source analysis followed by the separately authorized isolated native
experiment in `tools/terminal-feasibility/RESOURCE_CONTRACT.md`. No pin, production
dependency, policy or application engine changed. Prior C/Rust exporter receipts
do not verify the allocator patch; its own receipt is required.

## Recommendation

Pursue a narrow native MIT extension contract: creation-time **page allocator
injection** plus **exact history-prefix pruning**. Preserve 1,000 retained physical
history rows, 120×240 geometry, eight terminals and the 16-KiB control-string policy.
Keep the current engine until allocation-failure, pruning/reflow and controlled Mac
workloads pass. Do not treat approximate history preferences, RSS polling or a
counting general allocator as enforcement.

This need not wait for an upstream merge: an original, reviewed, separately hashed
MIT patch set against the unchanged upstream base pin is a maintainable experiment
option. Upstream acceptance would reduce maintenance; it is not a prerequisite for
an isolated spike. The subsequent opt-in experiment implements this narrow patch; it does not select
an application engine.

## What the pinned source actually provides

Review used Ghostty `22fa134a07f5749edd9e92c6bc17d8aaf61843be`, from the already
checksum-verified archive, and current Zmux terminal-adapter/session owners.

- The [public Zig module](https://github.com/ghostty-org/ghostty/blob/22fa134a07f5749edd9e92c6bc17d8aaf61843be/src/lib_vt.zig)
  exports `Terminal`, `Screen`, `PageList`, `TerminalStream` and `RenderState`.
  Its API is explicitly unstable, so an original Zig companion is possible at this
  exact pin without reverse-engineering an opaque C handle.
- [`Screen.eraseHistory`](https://github.com/ghostty-org/ghostty/blob/22fa134a07f5749edd9e92c6bc17d8aaf61843be/src/terminal/Screen.zig#L1720-L1731)
  accepts an optional inclusive bottom bound and refreshes cached cursor state.
  Its [`PageList` implementation](https://github.com/ghostty-org/ghostty/blob/22fa134a07f5749edd9e92c6bc17d8aaf61843be/src/terminal/PageList.zig#L5521-L5655)
  physically removes a prefix, including partial-page rows, updates tracked pins,
  row counts and viewport position. This is the primitive for exact oldest-N
  removal; the C API does not currently expose it. Prefer the Screen owner over
  calling its PageList directly and leaving cursor caches stale.
- `PageList.MemoryPool.init` already accepts separate general and page allocators.
  However, normal PageList construction, cloning and reflow select the
  [native OS page allocator](https://github.com/ghostty-org/ghostty/blob/22fa134a07f5749edd9e92c6bc17d8aaf61843be/src/terminal/PageList.zig#L566-L585).
  The general `GhosttyAllocator` does not intercept those native mappings.
- Four page items are preheated during initialization. Pool free lists retain
  mappings; compression can decommit physical pages while retaining virtual space.
  [`terminal/mem.zig`](https://github.com/ghostty-org/ghostty/blob/22fa134a07f5749edd9e92c6bc17d8aaf61843be/src/terminal/mem.zig)
  assumes dedicated, zeroed, page-aligned/page-multiple mappings. Injecting a generic
  malloc allocator or swapping the allocator of an already populated pool is unsafe.
- `grow` and limit enforcement currently prune whole historical pages, not exact
  lines. A post-feed trim alone permits transient excess inside that feed. Resize
  has old/new page and cursor-style ownership transitions to account for separately.
- Current vt100 has a fixed 32-byte cell with a 22-byte text array, so ordinary cell
  suffixes are also bounded. Zmux's existing structural limits are real, but they
  are not a measured or explicitly enforced whole-process RSS/footprint budget.

## Implementable choices

| Option | Honest enforcement and cost | Decision |
| --- | --- | --- |
| Existing C API, clamp exposed history and poll memory | Can restrict some UI/capture output, but cannot remove exact retained rows, stop search/reflow rediscovery, intercept OS page allocations or prevent transient memory spikes. | Reject as a solution to the current blocker. Keep only for the isolated exporter. |
| Original companion using public Zig API | Can own a Zig Terminal and use Screen prefix erase without changing engine source. Trimming between feed/resize calls gives a between-call row guarantee only. No creation-time native page allocator injection is exposed through the reviewed construction path. Requires its own effects/render facade; never cast the existing opaque C wrapper to a Zig Terminal. | Useful smallest **pruning-primitive spike**, not a complete resource solution. |
| Small native API/implementation extension | Thread a page allocator from construction through both screens, clones/reflow/resize and all page pools; enforce strict retained-row policy at history growth/reflow commit points using existing prefix-removal machinery. Keep general and mapping budgets separate, checked before allocation. | Recommended native path: bounded original patch surface, no new VM runtime and reuse of tested engine storage operations. |
| Unmodified native helper process with deadlines | A supervisor can kill a stuck helper and bound IPC queues/time spent waiting. Memory sampling followed by kill is reactive. Hosted Mac rejected `RLIMIT_AS`; CPU/core limits do not cap page memory. Linux delegated cgroup memory/swap controls are a possible OS-specific process-tree budget, not established here or a Mac solution. | Useful crash/time containment, insufficient portable hard-memory enforcement alone. |
| Wasm guest in a bounded runtime | Pinned source builds Wasm and its page pool grows linear memory directly. A runtime-enforced maximum linear memory plus fuel/epoch interruption can bound guest memory/instructions across macOS/Linux. Does not itself give exact retained-row pruning, bound host runtime/JIT/stack/IPC memory or provide physical key/display latency. The pin's Wasm target is browser-oriented, so native-host ABI/import glue and runtime/license evaluation are additional work. | Fallback if native enforceable-allocation changes are unsuitable; measure before accepting runtime/copy/packaging cost. |

Do not select a native freestanding target merely to reach the embedder allocator:
it changes target/runtime/link behavior and is not a drop-in macOS/Linux build mode.
Do not intercept malloc/mmap globally or replace a private page allocator after
construction; these routes depend on internal layouts and can corrupt ownership.

## Narrow native extension contract

1. Accept validated allocator/budget options **before** the first terminal/page-pool
   allocation. Old construction retains upstream defaults. Every primary/alternate,
   resize/reflow clone and temporary page allocation inherits the same owner budget.
   Exclude graphics/snapshot decode and omit clipboard/external-effect callbacks.
   Do not call compression or optional formatter/search/selection APIs in the first
   proof; compression is not excluded by a compile-time feature flag.
2. Use a fixed-capacity arena or checked allocator for general engine storage and
   a dedicated OS-page allocator with checked page-rounded reservation limits.
   Reserve quota before mapping, charge free/preheated and temporarily relocated
   mappings until actually released, and fail before exceeding it. Supply zeroing,
   alignment and decommit/recommit guarantees on both macOS and Linux. Fixed small
   frame/effect/queue budgets remain separate; include temporary copied frames.
3. Exact pruning keeps the newest 1,000 physical history rows. Conceptually remove
   the oldest `history_rows - 1000` prefix via the Screen primitive when positive.
   Enforce at each history-producing operation and reflow commit, rather than
   once per large feed or only before export. Verify terminal queries, tracked
   selections/search, cursor caches and viewport anchors cannot see stale removed
   content. Reflow scratch copies must fit the mapping quota even when the final
   retained history fits; no silent quota increase to finish a resize.
4. A quota denial or irrecoverable partial processing produces a typed failure,
   freezes the last good frame and requires explicit session recovery. No partial
   feed replay, automatic engine fallback or hidden history reset. Test every
   allocation failure path and destructor/accounting recovery.

The enforceable claim would be **audited engine-owned requested/reserved bytes**,
including pool/temporary storage, not total app RSS or physical footprint. Native
allocator metadata, code/stacks, AppKit/WebKit, PTYs and the kernel are outside that
claim and need their own accounting/workload acceptance. Hard wall cancellation
also needs a helper/supervisor boundary; a native C call on an in-process worker
cannot be forcibly cancelled safely. Name each guarantee precisely.

## Next experiment and real dependencies

The next bounded experiment can be chosen autonomously: an original public-Zig
companion proves exact prefix erasure on 1/1,000/1,001-row boundaries, narrow/widen
and height changes, partial-page removal, tracked references, scroll anchors and
continued cursor output. This validates the existing primitive, not a strict
automatic policy or a memory cap. A separately authorized native budget extension
then tests construction/preheat/clone/reflow allocation denial and zero outstanding
owned mappings after drop on Linux and Mac. No production migration follows from
either isolated proof alone.

We can keep pins/capacity fixed, choose the native experiment, design original MIT
interfaces and retain a reproducible patch inventory without an external service or
upstream maintainer decision. A new measured numeric memory/latency envelope must
come from the baseline campaign; no arbitrary MiB value is invented here. Actually
needing higher geometry/history/session limits is a separate product decision.

Real outstanding evidence needs: named Mac workload access, Intel runtime where
claimed, complete linked-runtime/Unicode notice closure, and executed failure tests.
If choosing Linux cgroups, actual delegation/administrator support and controller
verification are external deployment requirements; this session has no writable
root cgroup subtree. A portable Wasm alternative would need a separately evaluated,
pinned permissive runtime and host-memory budget. Upstream merge, signing and
notarization are not prerequisites for the isolated local experiment; release
signing remains a later distribution gate.


## Experimental implementation status

The original native option is now implemented behind the isolated runner's
`--resource-experiment` switch. See the
[F04 evaluation](f04-engine-evaluation.md#original-mit-allocator-and-exact-history-experiment)
for independently reviewed real-engine negative controls, exact-history and quota
failure/cleanup evidence. This does not select an application engine or clear Mac
resource, complete effect/fidelity, distribution-notice or measured workload gates.
