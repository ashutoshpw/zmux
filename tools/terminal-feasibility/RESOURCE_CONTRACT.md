# Experimental resource contract

Frozen design, 2026-10-15. Original MIT integration experiment against upstream
`22fa134a07f5749edd9e92c6bc17d8aaf61843be`; no production migration or pin change.
The earlier hosted exporter receipt at `3ac6fa4` is **pre-patch** evidence.

The opt-in resource experiment applies a separately hashed original patch to a
fresh checksum-verified source extraction. Default harness runs remain unpatched.
Existing upstream MIT notices remain intact; the patch and shim are original MIT.

## API and guarantees

- An experiment-only C entrypoint accepts general and page allocator interfaces,
  geometry and an exact physical-history-row maximum before initialization. Existing
  upstream constructors keep their default behavior. No opaque-handle layout casts.
- Dedicated mapping allocators provide zeroed, aligned OS mappings, not suballocated
  malloc memory. Charge page-rounded mappings before allocation and keep preheated,
  cached/free and transient old/new mappings charged until released. General and page
  quotas share an aggregate limit; failure is sticky and visible independently of
  the engine's informational VT error bit. All four allocator operations preserve
  current-length/alignment ownership. Unknown frees never unmap foreign memory.
- The strict row policy runs at Screen-owned history growth and resize completion,
  with prefix removal through `Screen.eraseHistory` and cursor/cache reload. At each
  completed atomic engine operation, retained history is at most the configured
  maximum, including during fragmented feeds. Do not substitute once-per-feed or
  once-per-export pruning. Reflow scratch copies may temporarily hold old/new data,
  but all mappings remain under quota. This is not a claim that every internal
  instruction transiently contains at most N rows.
- Map budgets bound audited engine-owned reserved/requested storage. Fixed host
  ledger/owner storage and copied frames have separately stated bounds. This does
  not bound total application/helper RSS, kernel metadata, AppKit/WebKit, stacks,
  shared libraries or wall time. Quota values in fixtures are experiment settings,
  not proposed production budgets or capacity increases.
- At return from a feed/resize/clone call, any recorded quota denial makes the
  owner permanently unavailable to subsequent consuming/export calls. Upstream may
  continue processing the remainder of the current at-most-4,096-byte feed. No
  instruction-level interruption, consumed-byte receipt or last-good-frame
  generation is implemented; there is no replay, transparent fallback or hidden
  reset. Partial construction and every normal/failure teardown must return live
  mappings/charges to zero. Preserve original raw/exporter fixtures.

## Ownership-path inventory

| Path in the exact pin | Required propagation/check |
| --- | --- |
| `terminal/c/terminal.zig:new_` → `Terminal.init` | Dedicated new entrypoint supplies options before Terminal/ScreenSet/page preheat; original constructor unchanged. Wrapper/parser/effect storage uses bounded general allocator. |
| `Terminal.init` → `ScreenSet.init` → `Screen.init` → `PageList.init` | Primary receives both allocator and strict-row policy. Page preheat, pool free-list/node/pin bookkeeping and initial viewport are charged. |
| `Terminal.switchScreen` → `ScreenSet.getInit` | Lazy alternate inherits the primary owner allocator and exact zero history. Returning to primary preserves its policy and budget. |
| `Terminal.resize` alternate replacement after primary resize | Replacement alternate inherits the same page allocator; no default-allocator fallback on either success or failed allocation. |
| `Screen.clone` → `PageList.clone` | Preserve strict policy and source page allocator for copied storage. General clone/remap bookkeeping remains charged to its supplied bounded allocator. |
| `PageList.resize`/`resizeCols`/temporary clone/recycle paths | Standard and nonstandard page creation uses `pool.pages.allocator`; old/new pages and temporary relocation peaks stay charged. No allocator swap on live pools. |
| `PageList.Builder.init`/finish | Optional injected page allocator propagates from Builder options. Snapshot decode/import does not currently expose bounded construction and is unsupported by this experiment until explicitly wired/tested. |
| `Screen.cursorDownScroll` and `cursorScrollAbove` | These are the two reviewed Screen callers of `pages.grow`; strict pruning happens after their cursor/style/reference transitions complete. |
| `Screen.resize`, clone and reset/clear | Reconcile exact physical-row cap after resize; clone retains policy; reset clears data without replacing pool owner. Validate active/alternate transitions and continued output. |
| C rendering/iterator helpers | If used in the resource fixture, create with the same bounded general allocator. Caller-owned snapshot byte/cell buffers remain separately bounded. |
| Optional compression/images/clipboard/snapshot decode | Graphics and snapshot decode are excluded by the minimal features; clipboard effects have no callback. Compression is not invoked (it is not a compile-time feature exclusion). Search/selection/formatter APIs are not called. None of these optional paths has allocation-failure acceptance in this proof. |

## Independent fixture acceptance

Constructor denial across each observed allocation position; low page budget before
preheat; aggregate/general/page denial during feed, lazy alternate and resize/reflow;
exact 1/1,000/1,001 boundaries and retained newest-row contents; byte-fragmented VT
feed, narrow/widen/height changes, primary/alternate/reset and continued cursor output;
no quota overshoot and zero live mapping/charge ledger after every teardown. Capture
red before the policy/allocator repair and green afterward. Record patch/shim hashes,
base source, features, allocation-path exclusions and local versus hosted results.

No staging until parent coordination. Current `test_runner.py` is owned by the
browser runner repair slice; do not edit it in this experiment.

## Allocation audit and exclusions

Reviewed the pinned Terminal/Screen/PageList construction, growth, clone, resize,
reset, lazy alternate/replacement, C stream wrapper and render helper paths. Both
`MemoryPool.init` sites plus PageList clone inherit the injected page owner. Normal
page creation/replacement/reflow uses that pool; preheated and cached mappings
remain charged. Terminal/parser/stream scratch and the render helpers use the
supplied general allocator. The standalone shim has a fixed 4,096-mapping ledger;
its Resource and Probe owners occupy at most `sizeof(Resource) + 8192` bytes (Probe
has a compile-time size guard). Each copied fixture snapshot is at most 512 KiB and
is host memory outside the engine quota. Mapping/accounting metadata in the kernel,
libc/runtime/IO/logging globals, stacks and test input Strings are excluded.

This is an audited closure for the exercised minimal API, not a proof covering all
upstream exports or arbitrary future feature combinations. In particular Builder
propagation is source-reviewed only; bounded snapshot import is unsupported, and
compression strict-decommit/restore has not been exercised. Ordinary pool
zero-decommit/recommit is source-audited and exercised indirectly on Linux by page
creation/recycling; there is no independent syscall-success or physical-reclaim
measurement. The dedicated mapping shim
permits the required zeroing/alignment ownership but makes no measured physical
footprint claim. Existing app dependencies and its exact structural policies remain
unchanged.

The fallback branch replacing the alternate after failed in-place resize is
source-reviewed, not forced by the success fixture (which resizes the existing
alternate). The partial top-region `cursorScrollAbove` rotation branch is likewise
source-reviewed; current ordinary newline fixtures use its bottom fast path.
These remain distinct unexecuted branch-coverage gates, including failure cleanup.
