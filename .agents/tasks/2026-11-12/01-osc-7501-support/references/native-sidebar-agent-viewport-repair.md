# Native sidebar Agents viewport fixture repair

**Date:** 2026-11-13 UTC
**Source reviewed:** `848e72c75d5c06eefaba7c95b08c5c24b4318c32` on `main`
**Scope:** Existing `sidebarGroupingSmoke` expanded-stage fixture sizing and
this reference only. No production sidebar policy, window minimum, provider
selection, Programs behavior, task plan/status, workflow, or app operations.

## Repair

After the existing per-row reveal loop has found and validated all three real
lifecycle rows and expanded groups, the smoke reads the larger of
`AgentSidebarList.fittingSize` and its laid-out frame height. It requires that
height to be finite and positive, then adds a local required minimum-height
constraint to the existing Agents `NSScrollView`. The existing 28% Agents and
Programs proportional constraints remain installed. The temporary minimum is
scoped with `defer` so every throw removes it and relays out the content. On the
successful path it is explicitly deactivated and the content is laid out before
the existing compact 900×420 stage.

The existing individual Agent row reveal and unchanged-Spaces-origin checks,
three-row simultaneous visibility condition, exact group-label set, and
expanded Agents screenshot remain. The compact overflow check and all later
exact-provider selection, Programs owner/epoch, and surface/session/PTY
inventory checks remain in place. Bounded failure geometry reports only counts
and rectangles for the Agents viewport/document/rows/groups, the Programs
viewport, and the Spaces-origin result. It emits no owner IDs or terminal text.

**The expanded Agents image uses this temporary fixture viewport. It is not
evidence that all three groups fit under the normal 28% production ratio.** The
constraint is removed before compact mode; production split policy and the real
window minimum are unchanged.

## Hosted red evidence

Before this fixture repair, universal run `43441181931` at the exact source SHA
first failed the packaged native sidebar smoke with
`Populated Agents rows or grouped state headers are incomplete`. The
failure-only screenshot showed the top Agent content clipped with Working and
Idle visible; the Programs section remained expanded. The hosted log and
diagnostic artifact are recorded in
[hosted-final-848e72c.md](hosted-final-848e72c.md).

At the requested 760-point content size, the sidebar reserves 32 points for
chrome, 4 for the gap, and 54 for the footer, leaving at most 670 for sections.
The normal Agents ratio allocates 187.6 points. The three rows span 200 points
from first to last; the complete three-group document is 240 points from three
24-point headers, three 48-point rows, five 2-point stack gaps, and 14 points
of list inset. The observed failure therefore establishes the original
expanded fixture's geometry mismatch. The original run did not retain numeric
AppKit frame/clip rectangles, and Linux cannot reproduce that AppKit execution.

## Test-authoring gate

1. **Observable contract:** In the expanded packaged sidebar smoke, all three
   real reconciled provider rows and their expanded group labels must be
   simultaneously visible while Programs remains expanded. The same smoke must
   still prove compact independent Agents scrolling and preserve exact provider
   and OSC 7501 selection behavior.
2. **Credible regression:** The prior 28% viewport at the requested expanded
   size was smaller than the three-row span, so sequential per-row reveals
   passed while the simultaneous visibility guard failed. The exact hosted
   failure and screenshot are retained as red evidence.
3. **Coverage boundary:** Only the packaged AppKit smoke owns this split,
   document, and clip-view geometry. Rust/Foundation checks do not exercise it.
   Keep this smoke as the single owner and retain all downstream selection and
   Programs assertions; no duplicate test was added.
4. **Production seam:** None. The fixture uses the populated list's measured
   size and its existing enclosing scroll view. The temporary constraint is
   local to the smoke and is removed before compact mode; there is no new
   production API, ratio change, window-minimum change, or test hook.

## Validation and limits

- `git diff --check` passed for the tracked Swift diff. The bounded source
  inventory check `python3 -B tools/native-idle-workloads/run.py --check-inputs`
  passed with 402 inputs and `execution=false`; it does not compile Swift or
  exercise AppKit.
- `python3 -B scripts/check-macos-sources` exited at its platform guard with
  `Native source checks require macOS and its SDK`. This host reports Linux and
  has no `swift`, `swiftc`, or `xcrun`, so native Swift typecheck and packaged
  sidebar execution are unavailable here.
- The packaged contract `scripts/check-native-sidebar` was not run: it launches
  and exercises the app, which is outside this slice. Source review confirmed
  the later exact provider selection, Programs owner/epoch, and inventory
  assertions are unchanged.
- No `scripts/check`, app build/run, workflow, commit, push, or task plan/status
  update was performed by this slice.

## Parent review resolution

The independent review accepted the fixture design and noted a P3 evidence gap:
the post-growth Spaces origin appeared only in failure geometry. The parent
added the unchanged-Spaces-origin equality to the expanded success guard, after
the temporary minimum has been applied. This retains the same invariant across
the viewport change and adds no new seam or duplicate test. Aggregate validation
is repeated for this final source. Native execution remains pending.

The final parent aggregate (`RUST_TEST_THREADS=2 scripts/check`, round 38)
passed **767 passed, 0 failed, 7 ignored** in 262.30 seconds, with unchanged
source fingerprints and preserved unrelated hashes. This includes the added
post-growth Spaces-origin success assertion. Native/hosted execution follows.
