# Independent native sidebar Agents viewport recheck

**Date:** 2026-11-13 UTC
**Base:** `848e72c75d5c06eefaba7c95b08c5c24b4318c32`
**Reviewed fixture diff:** `apps/macos/Sources/App.swift` (56 insertions, 2 deletions; working-tree blob `1db14ebd4690f382b70c6e0080b6e1ec008abd7e`)
**Scope:** `sidebarGroupingSmoke` expanded-Agent viewport around `App.swift:909-1025`, production sidebar constraints needed to judge it, and the diagnosis/repair references. No source, task status, plan, or other files changed by this recheck.

## Verdict

The fixture-only repair is sound at source level. The temporary constraint targets the existing Agents `NSScrollView`, derives a finite positive minimum from the populated list, and leaves the production 28% split constraints and window minimum intact. After layout, the smoke checks each real Agent row's actual `visibleRect`; `defer` cleans up on throws, and the success path explicitly removes the constraint and lays out before the compact stage. The diff does not weaken the original provider, group-label, selection, inventory, or Programs assertions and adds no production seam.

One non-blocking assertion gap remains: the unchanged Spaces-origin check covers the individual Agent reveals before the new constraint is installed, while the origin sampled after the grow is diagnostic-only. This does not establish that the origin changes; it limits what this smoke proves about that layout pass.

The expanded Agents screenshot uses the temporary fixture viewport. It is not evidence that the rows fit under the normal 28% production ratio. This recheck did not execute AppKit, Swift typechecking, the packaged smoke, or the Programs acceptance image.

## Findings

### P3 — Spaces origin after the expanded relayout is not a success condition

At `App.swift:935-942`, each individual row reveal checks that the Spaces scroll origin remains equal to its baseline. Those checks run before the temporary minimum is activated at `App.swift:962-971`. The post-layout success guard at `App.swift:985-986` checks simultaneous full-row visibility and the exact group-label set. `spacesOriginUnchanged` is computed only inside that guard's failure branch at `App.swift:1009-1017`, so a successful expanded stage does not assert the origin after the sibling layout has been resized.

This is an evidence gap, not evidence of a production defect or observed scroll. If independence must include this fixture relayout, include the origin comparison in the success guard. Otherwise keep the claim limited to the existing per-row reveal behavior.

## Review evidence

- Production layout pins the Spaces, Agents, and Programs sections in `App.swift:458-479`. The Agents and Programs 28% height constraints are `.defaultLow` at `App.swift:511-516`; the fixture's `.required` lower bound applies only to the existing Agents scroll view. At the requested 760-point content size, the diagnosis calculates up to 670 points for the sections. The 240-point populated Agents document plus the existing section headers and scroll-view minima requires 442 points, so the source constraints leave room at that requested size. The earlier failure analysis and its screen-scale limit are recorded in [the diagnosis](native-sidebar-agent-viewport-diagnosis.md#layout-arithmetic).
- `App.swift:956-971` uses `max(fittingSize.height, frame.height)`, rejects non-finite/non-positive results, and installs the local constraint after the individual reveals. Following layout, `App.swift:973-977` checks all three rows against their actual clip-derived `visibleRect`s. The exact three expanded group labels remain in the positive guard at `App.swift:978-986`; visible-group counts and the bounded row/group rectangles are collected only for failure reporting at `App.swift:987-1017`.
- Cleanup is registered immediately after constraint construction at `App.swift:966-969`. It runs on later throws. On success the constraint is explicitly deactivated and layout is flushed at `App.swift:1019-1024`, before resizing to 900×420. The compact overflow assertion remains at `App.swift:1025-1029`.
- The diff only adds viewport sizing, diagnostics, and the `agentScroll` binding. The exact Agent-owner and selection/inventory checks after compact mode remain at `App.swift:1032-1175`; Programs owner, nonfocused summary, independent reveal, selection, and inventory checks remain at `App.swift:1177-1268`. The repair reference records these retained contracts at [native-sidebar-agent-viewport-repair.md](native-sidebar-agent-viewport-repair.md#repair).
- Failure diagnostics have fixed expected-row/group inputs: at most three row rectangles and three expected group rectangles, plus counts, viewport/document rectangles, and a Spaces-origin boolean. They do not serialize owner identifiers or terminal text.
- `MainWindowHost` is `@MainActor` at `App.swift:62-63`; the constraint and AppKit layout operations remain within that isolation. The `CGFloat` heights feed `max`, `isFinite`, and the constant-height constraint with matching types. No Swift 6 actor or type issue is apparent from source inspection.

## Proof limits

The earlier hosted failure screenshot and its missing numeric AppKit geometry are documented in [the diagnosis](native-sidebar-agent-viewport-diagnosis.md#exact-hosted-failure). The repair note records that native checks require macOS and that no post-repair packaged sidebar/Programs run was performed in that slice. No accepted post-repair Agents or Programs image is available in the reviewed evidence. The parent's `scripts/check` is running separately; this recheck did not start another check, build, workflow, or app operation.
