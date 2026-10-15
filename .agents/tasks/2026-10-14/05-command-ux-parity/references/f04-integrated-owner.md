# F04 single resource/effect owner

2026-10-15. Original MIT, isolated harness work authorized after separate resource
and effect acceptance. No app dependency/engine switch, upstream pin change or
additional upstream patch. Existing `resource.patch` and contract remain unchanged.

## Ownership and reuse

`Resource` remains the only Terminal/Probe owner and constructs exactly one terminal
through the existing bounded constructor. Extract the fixed callback collector from
`effects.c` into an original shared component with no terminal/parser constructor.
The old effects fixture keeps its default constructor and uses that same collector;
the old resource fixture keeps its ownership and negative controls.

An integrated outer owner embeds the collector and owns one opaque Resource. An
original Resource-owned binding function installs the collector context and public
WRITE_PTY/BEL/RESET callbacks after bounded construction and before first input.
It never exports or casts a private engine layout. Probe/render helpers use the
same bounded general allocator. The collector and allocator contexts remain stable
until Resource teardown finishes; only then are outer storage and contexts freed.
No callback allocates, reenters the engine, writes PTY/UI or acquires external locks.

## Admission, staging and publication

The owner is single-threaded with idle, staged, failed and closed phases. Input
validation and caller host-buffer reservation precede consumption. Each successful
feed/resize stages one monotonically identified result, including an empty callback
batch. No further consuming call is admitted until that exact result is committed.
Snapshot export and callback copy do not retire the staged batch. Insufficient drain
capacity rejects before copy; a validated commit retires it once without allocation.

Failure gates combine allocator denials/invalid ownership, explicit callback
reply/event overflow, reported VT/API failure and snapshot/export failure. Check
before/after the operation and **again after render/export**, since render updates
can allocate. A valid earlier BEL/reply/RESET never permits publishing a batch after
a later allocation denial. Upstream may finish the current <=4,096-byte feed; no
consumed-byte receipt, instruction interrupt, replay, fallback or hidden reset exists.
RESET neither clears failure/accounting nor restarts notice sequence identity.

Rust keeps preallocated candidate and last-good slots. Validate all text/event
ranges and UTF-8 before C commit; after commit, publishing is a nonallocating slot
swap. On failure the old slot stays published and further input/export is refused;
statistics/cleanup remain readable. External copies/retained frames are separately
bounded caller storage, not charged as though this owner controlled their lifetime.

## Accounting

The C managed budget is `fixed_owner_allowance + engine_reserved_mapping_limit`.
Compute fixed storage from the actual integrated struct (collector included once)
plus Resource/Probe allowance, before engine construction; subtract it from the
requested C budget. Do not sum two Probe allowances or create an Effects terminal.
Report engine mapped current/peak, fixed C allowance and their aggregate separately.
These are bounded requested/fixed storage plus reserved engine mappings, not RSS,
allocator/kernel overhead, stacks, libraries, physical footprint or total app memory.

Reserve two bounded Rust publication slots before consuming input: each has at most
512KiB snapshot bytes, 1,024 event records and 4,096 reply bytes, plus fixed cursor,
generation and 32 BEL notice-sequence records. Report actual buffer capacities
separately; do not hide infallible Vec/String/Published clones inside the budget.
Constructor capacity/allocation rejection happens before creating the engine. No
per-step host growth is needed. Caller-requested snapshot copies remain outside
that owner storage accounting. These experiment budgets do not set product limits.

## Independent acceptance

The new risk is composition: separate fixtures cannot prove quota-denied callback
batches are fenced or that post-render denial preserves last-good publication.
Use the real bounded constructor, collector and render owner, not a fake terminal.

- Exact reply/order/reset and 1,000-row retention in this same terminal, with
  fragmented feed, reflow, primary/alternate and continued output.
- Valid callbacks followed by general/page/aggregate denial in the same call;
  no frame/reply/notice generation publishes, no replay, zero engine mappings at close.
- A real render allocation denial after an otherwise successful step; verify the
  allocation trigger rather than assuming resize or rendering always allocates.
- Reply/event limit overflow and snapshot failure with the quota owner still live;
  last-good slot/sequence stay unchanged, all cleanup charges return to zero.
- Constructor denial at every observed allocation position; host-budget rejection
  before engine creation; stale commit, pending-result and short-drain refusal.
- Exact fixed+mapped accounting, owned snapshot lifetime and repeated teardown.
  Negative controls bypassing the post-render fence must fail the intended assertion.

The informational VT error bit still excludes some gracefully handled/optional
failures. This adds visibility for actual denials from this shared allocator, not
proof of every upstream export/capability or optional path. Title/cwd/OSC source,
real PTY delivery, AppKit fidelity, named-Mac workload budgets, optional compression
and production selection remain independent gates. First run locally with private
build output and hashed inputs, obtain independent review, then request the parent's
hosted workflow execution. Do not stage/commit without parent coordination.


## Implementation refinement before acceptance

The shared collector is `collector.c`/`collector.h`; the integrated owner uses the
original `resource-internal.h` binding and snapshot interface. Its C managed budget
includes one Resource/Probe allowance plus the outer collector (155,952 bytes on
this Linux host), followed by the reserved engine mappings. Rust separately reserves
two publication slots: 1,081,344 backing capacity bytes plus 800 bytes of inline
owner metadata on this host. The constructor reports host budget/allocation and
nullable C construction errors through `Result`; slot reservation precedes engine
construction. No step needs host allocation to validate/commit/swap publication.
Fixture-only owned String copies and their allocation are outside these owner slots.

History queries require idle. Scalar quota tightening may occur while a result is
staged **before** successful export: it validates all caps before assignment and
performs no engine/allocator operation. This permits a real post-step render-denial
fixture. Successful export seals further export, history, mutation and quota-change
calls until exact-ID commit; stats remain observational. Short drain capacity is
retryable before the seal. Failed/stale commit never retires the batch. This keeps
the nonallocating commit interval free of engine or budget changes.

The local first full run already exercised all prior separate fixtures and five
intentional negative controls. A new temporary original integrated-shim mutation
bypasses the render result/quota fence and demonstrably fails the named post-render
publication assertion; the underlying terminal and allocator are real. Final
current-hash evidence and independent review are required before staging.

## Final reviewed-source local receipt

`/tmp/zmux-f04-integrated-repro-final-20261015/receipt.json`, SHA-256
`0c0bd1483598c8ebc812ecaa68b8a027311076b1679a2798e15e1693ba219a30`, records
base HEAD `dfd99cc0abce03e91ab945d14f9bffe7d32fa214` plus 23 harness input hashes;
all match the final frozen source after completion. Earlier local receipt directories
predate the final export-seal hardening and are predecessor evidence.

The run downloads/verifies fresh source/toolchain archives, labels its explicit
existing Zig dependency cache and uses private temporary Zig/Cargo output. All
ordinary raw/adapter/compatibility/resource/effects/integrated stages pass. Five
intentional exit-101 controls fail their named assertions: two unchanged-constructor
resource controls, two temporary original-collector controls and the temporary
integrated render-publication control. Ghostty build takes 66.354 seconds; integrated
fixture 1.078 seconds under separately reported three-CPU-second/512MiB AS/no-core
Linux limits. These timings establish no performance budget.

The actual healthy-resize -> render-export denial increments the allocator denial
counter after the healthy stage; neither frame nor callbacks publish, and all engine
mappings return to zero. Fifteen constructor-denial positions, same-call callback
quota failures, reply/event/snapshot failure, exact1/1000 retention, reflow/reset/clone/
alternate and 128 lifecycles pass. Raw C boundary checks cover host allowance refusal,
nullable construction error, empty-batch staging, short drain retry, history while
staged, duplicate export, quota changes after sealing and stale/duplicate commit.
Parent architecture review accepted the ownership/fence direction; fallible C
construction and the sealed interval were hardened from its findings. Fresh independent
source/runtime review is pending. Nine runner process checks, strict C warnings,
Rust formatting, Python syntax and diff/link checks pass. Hosted integrated execution
and all preceding production/native/metadata/workload gates remain open.

## Independent acceptance and parent checks

Independent source review found no remaining blocker for this isolated boundary.
The complete rerun at `/tmp/zmux-f04-integrated-independent-review-20261015/receipt.json`
has SHA-256 `fcd796e0b1f08d98eac4fa2d8a4b9da9589ef14469771d06b1b87623d25b6cc3`;
parent rechecked all 23 input hashes. Thirty normal stages and all five intended
assertion-failure controls pass without timeouts. Linux-only execution and the
explicit dependency cache are recorded; native/production limits above remain.

Parent `scripts/check` passed in `/tmp/zmux-integrated-parent-verify-llt5l_v4`
(log `/tmp/zmux-integrated-parent-check.log`), whose 23 harness hashes also match.
The nine existing runner lifecycle tests pass independently. GitHub Actions gains
an opt-in integrated experiment that also enables the required resource experiment;
its hosted run remains pending.

## Hosted integrated acceptance

Run `38771957372` at `93daf02260f730a42a7e9370df42bd069dc01a50` passed
Linux x86_64 and macOS 15.7.9 arm64. Parent downloaded both artifacts to
`/tmp/zmux-f04-integrated-hosted-38771957372` and matched all 23 input hashes
against the commit. Receipts:
- macOS SHA-256: `4260078a54498043b393de65650de8e5c22ca6a87f124bf3151d03e131e22d5c`
- Linux SHA-256: `add7cacdd7940f1219ad9245357bc473fbe819b8210576b2ed4fcdbaadccc783`

Both execute the single-owner history/callback/quota/render-failure and teardown
fixtures. macOS reports RLIMIT_AS unavailable (ValueError); CPU/no-core limits are
configured. The shim's quota accounting remains independently exercised. The raw
library/probe compiled universally, while integrated execution is host-only. This
does not establish an application memory bound, real PTY delivery or app engine
replacement; metadata and native fidelity/performance gates remain open.
