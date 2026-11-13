# Native sidebar Agents viewport diagnosis at `848e72c`

**Date:** 2026-11-13 UTC
**Source reviewed:** `848e72c75d5c06eefaba7c95b08c5c24b4318c32` on `main`
**Scope:** Read-only diagnosis of the hosted Agents viewport failure and a
fixture-only proposal. This note is the only file written for this subtask.

## Verdict

The strongest evidence points to an unsatisfiable expanded-stage fixture
geometry requirement under the current sidebar split policy. The smoke requests
a 900×760 content area, but the Agents viewport receives only 28% of the
`sidebarSections` height. At that requested size this is at most 187.6 points.
The three provider rows span 200 points from the first row's top to the third
row's bottom, and all three group headers and rows occupy a 240-point document.
The final simultaneous-row assertion therefore cannot pass at the requested
size under the existing constraints. The failure screenshot is consistent with
that calculation: the top provider content is clipped while Working, Idle, and
the expanded Programs section are visible.

This is a fixture/layout mismatch; it does not establish a defect in lifecycle
projection, provider ownership, selection, or the production scroll behavior.
The exact runtime window and clip-view dimensions were not written to the
failure receipt, so a display-height clamp is plausible but not proven. The
source calculation is enough to show that merely requesting 760 points cannot
satisfy the current simultaneous visibility condition.

## Evidence

### Exact hosted failure

The current task record, [hosted-final-848e72c.md](hosted-final-848e72c.md),
records Foundation passes of Ubuntu **767/0/7** and macOS **765/0/7** at this
SHA. Universal run `43441181931` passed the Cloud lifecycle and packaged
app/native smoke, including `file_panel=true`, then first failed in the native
sidebar/status step with:

```text
Native sidebar smoke failed: BridgeFailure(message: "Populated Agents rows or grouped state headers are incomplete")
```

The full log is
`/tmp/zmux-osc7501-final-848e72c/universal-development-job-131940000318-full.log`.
The extracted `native-sidebar-agent-fixture.json` says `success=true` and
contains accepted lifecycle observations for `claude_local` (working),
`codex_local` (waiting), and `opencode_local` (idle), each with an exact
window/workspace/pane/surface/session owner and generation. The app reached the
aggregate visibility guard after its loop had found each expected row and
expanded group and independently scrolled each row fully into view without
changing the Spaces scroll origin (`App.swift:913-949`). The error combines the
final all-row visibility and exact group-label checks, so the log does not name
which boolean failed. The earlier per-row guards and the image make simultaneous
visibility the supported explanation; no per-rectangle failure receipt exists.

The failure image is
`/tmp/zmux-osc7501-final-848e72c/native-diagnostics-extracted/zmux-sidebar-failure.png`
(900×684 pixels). It shows the top Agent group/row clipped, Working and Idle
rows visible, and Programs still expanded with its terminal group and two
program rows. `captureSidebarImage` renders `window.contentView.bounds`
(`App.swift:1362-1367`), but the PNG does not include AppKit point scale, the
window frame, or the screen's visible frame. It is visual evidence of clipping,
not a numeric window-geometry receipt. No `native-sidebar.json`,
`native-sidebar-programs.json`, or `zmux-sidebar-programs.png` was produced, so
the failure screenshot is not Programs acceptance evidence.

### Layout arithmetic

- The window minimum is a **680×420 frame** (`App.swift:180`). The sidebar
  reserves 32 points for chrome, a 4-point gap, and a 54-point footer
  (`App.swift:441-457,485-488`). Thus `sidebarSections` has at most
  `760 - 32 - 4 - 54 = 670` points when the requested 760-point content size is
  honored.
- `agentScroll` has a low-priority height constraint equal to 28% of
  `sidebarSections` (`App.swift:511-516`), yielding at most `670 × .28 = 187.6`
  points at that requested size. `Programs` has a separate 28% constraint.
- Each populated Agent group header is 24 points and each row is 48 points
  (`AgentSidebarViews.swift:421-446,504-535`). The six arranged views have five
  2-point gaps; the list adds 4 points above and 10 below
  (`AgentSidebarViews.swift:184-189,307-327`; `App.swift:355-357`). The full
  document is `3×24 + 3×48 + 5×2 + 4 + 10 = 240` points. Even the row-only
  first-to-last vertical span is 200 points because two intervening group
  headers and their stack spacing separate the rows. Both exceed 187.6.
- If the 684-pixel failure bitmap is one pixel per point, the observed content
  height would yield 594 section points and a 166.3-point Agents viewport. That
  is consistent with the image, but pixel scale was not recorded, so this is an
  estimate rather than a measured AppKit value.

The ratio policy does not show a production defect by itself: Agents and
Programs are separate scrolling sections, and the same smoke deliberately
requires the compact stage to exercise independent Agents scrolling
(`App.swift:968-975`). The defect in this fixture stage is asking for all three
rows to be visible at once without allocating enough height to that scroll view.

### Existing invariants and prior viewport repair

The current smoke preserves strong owner checks before and after this guard:

- Three lifecycle records must have the expected provider, activity, connected
  state, exact terminal owner, and prompt (`App.swift:861-905`). The acceptance
  projection records each provider's runtime, generation, and provider-session
  IDs (`App.swift:1267-1295`).
- Compact mode must produce real Agents overflow (`App.swift:968-975`). Each
  provider is then revealed in the Agents scroll view while the Spaces clip
  origin remains unchanged; selecting each row must focus its exact existing
  provider/terminal target and leave the complete surface/session/PTY inventory
  unchanged (`App.swift:1041-1120`).
- Programs must retain the exact OSC 7501 owner and both focused records,
  preserve the nonfocused priority summary, reveal that row in the independent
  Programs scroll view without moving Spaces, select the exact owner, and leave
  the complete inventory unchanged (`App.swift:1123-1215`). These checks occur
  after the failing aggregate guard and have not executed in this run.

The prior [workspace drag viewport repair](workspace-drag-viewport-repair.md)
found that an artificially reduced live window minimum made a real sidebar
destination unreachable. It now captures the app's actual minimum and uses
`contentRect(forFrameRect:)` to size the fixture (`WorkspaceOrderSmoke.swift:269-275`).
Do not transfer its former 180-point minimum or 270-point content probe to this
smoke. Keep the real window minimum and the independent Spaces/Agents/Programs
scroll views.

## Smallest robust remedy proposal

Keep the normal production ratio, all populated records, the expanded Programs
section, and every current owner/selection assertion. In the expanded smoke
stage only, derive the populated `agentSidebarList` document height after
reconciliation and layout, then temporarily require its enclosing Agents
scroll view to be at least that tall. A local constraint in the existing smoke
can use `agentSidebarList.enclosingScrollView`; it does not require a new
production API or a persistent layout preference. With a 240-point document,
the minimum section stack remains feasible: three 28-point section headers,
56-point Spaces scroll minimum, 240-point Agents viewport, and 62-point
Programs scroll minimum sum to 442 points. Even a 594-point section would have
152 points of slack. The existing Programs ratio remains active and Programs
stays expanded and visible.

Retain the existing per-row reveal and unchanged-Spaces-origin checks, exact
three-provider/group-label condition, and simultaneous full-row condition. A
full-document viewport also makes the three group headers visible together.
Capture the expanded Agents image with Programs still present, then deactivate
the temporary constraint and lay out again before resizing to 900×420. The
compact overflow guard and all later provider-selection and Programs
owner/generation assertions remain in their present order and unchanged.

This proposal is fixture-only and awaits the parent's acceptance before any
implementation. Do not enlarge the requested window to an arbitrary value,
collapse Programs, lower the live minimum, reduce expected rows/groups, remove
visibility checks, or change the production split policy based on this failure.

## Test-authoring gate for that fixture change

1. **Observable behavior:** The packaged native smoke must display all three
   exact provider rows and their expanded status groups simultaneously while
   the Programs section remains present, then retain its compact independent
   scroll and selection journeys.
2. **Credible regression:** A 28% Agents allocation smaller than the rows' span
   lets each row pass an individual scroll check but makes the final simultaneous
   visibility guard fail. The current source arithmetic and failure image
   demonstrate this case.
3. **Coverage boundary:** Rust/Foundation tests do not own AppKit split sizing,
   `NSScrollView` clipping, or the packaged sidebar smoke. This existing native
   smoke is the owner. Keep its provider selection, program selection, exact
   owner, and inventory checks; do not add a duplicate lower-layer test.
4. **Production seam:** None is needed for a local constraint attached to the
   existing Agents scroll view and removed before compact mode. Do not add an
   exported layout hook, test flag, or production behavior solely for the test.

## Bounded geometry evidence if runtime confirmation is required

The source and image establish the fit mismatch, but they do not identify why
the requested 760-point content height renders as a 900×684 bitmap. Before any
layout-owner change, one bounded failure-only geometry record at the expanded
guard can distinguish the display clamp from internal allocation. Record, at
that point only:

- requested content size; actual `contentView.bounds`, `window.frame`,
  `window.minSize`, `window.screen.frame`, and `window.screen.visibleFrame`;
- `sidebarSections.bounds`, all three section header frames, and the Spaces,
  Agents, and Programs scroll frames and clip bounds;
- the Agent document frame and each expected group's and row's bounds
  converted into the Agents clip-view coordinates, with each `visibleRect`;
- Agents and Spaces clip origins immediately before the aggregate guard.

This is enough to verify the 28% allocation, simultaneous row span, group-header
visibility, continued Programs allocation, and separation from Spaces without
adding polling, changing owners, or rerunning any provider. It should be one
failure receipt, not ongoing logging. No geometry probe, source edit, test,
check, build, workflow, app operation, commit, push, or index change was made
for this diagnosis.
