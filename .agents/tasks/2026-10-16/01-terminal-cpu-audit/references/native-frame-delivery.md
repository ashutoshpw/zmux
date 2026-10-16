# Native presentation repair

Implementation after notification slice53a98b5. Original MIT code; no cmux
renderer, assets or implementation translated/imported.

## Transport

New C ABI `zmux_desktop_poll_frames(desktop, revision)` returns protocol1:
metadata snapshot with empty frames plus ordered frame packets. Full
`zmux_desktop_snapshot` and CLI/control snapshot behavior remain unchanged.

Each packet carries current session/process/start/failure metadata. A full reset
contains its complete terminal snapshot. An unchanged snapshot omits the grid;
a changed snapshot supplies only changed drawing rows and a UTF-8 text replacement
span, with complete bounded cursor/mode/copy metadata. Unicode cell origins and
styles are preserved; glyphs are not concatenated to manufacture fewer draw calls.

The bridge keeps two baselines of selected frames only, not historical desktop
models or hidden control frames. Baselines are namespace/revision scoped; unknown
or expired baselines get full resets. Buffers are reserved before cache changes,
normal ABI capacity/release rules apply, and no terminal command is replayed.
Swift validates/merges into a temporary cache and advances revision only after all
frames merge. Frame count is bounded by the model pane count across windows; row indices and
UTF-8 replacement spans are validated.
Clipboard/selection/current-window projections receive complete assembled frames.

## Drawing and chrome

TerminalView compares visual rows/cursor/state instead of assuming every bytes-read
increment changes pixels. It invalidates changed rows and visible cursor regions;
draw skips rows outside the dirty rectangle. Fixed font metrics and four font trait
variants are cached; the style/color attribute cache is bounded at256 entries.
Unchanged contents/title do not rewrite accessibility values.

Workspace rows, header, unread icon, tab labels and group counts gate presentation
on their own inputs. Initial presentation and real metadata/attention changes are
retained. Font remains the existing fixed point size; any future configurable font
must invalidate these caches. No repaint throttling or PTY/notification processing
is disabled to achieve this reduction.

## Verification

- Real C ABI consumer test: two PTYs, exact Unicode output, only changed row0,
  unchanged second pane without grid, scalar-aligned text merge, unknown-baseline
  reset and compatibility of the full snapshot reader.
- Public NSView contract in existing native sidebar journey: byte-progress-only
  metadata requests no repaint; a visible cursor update requests repaint; repeated
  frame requests no repaint. The fixture restores its original presentation.
- The macOS workflow deliberately substitutes the older MIT TerminalView from
 53a98b5, requires the intended repaint failure and physical app cleanup, restores
  current source, then rebuilds/tests the product. This diagnostic variant is not
  published as an installable product. Actual hosted red/green remains required.
- Full local checks, including strict Clippy, notices and both C ABI suites, pass
  after selected-only baseline retention; log `/tmp/zmux-native-frame-retention-check.log`.

## Diagnostic throughput comparison

One sequential full-API/delta-API pass on the shared Linux host, same default
release bridge and controlled two40x120 PTYs as the earlier probe. Initial full
baseline is included. Neither run decodes full frames during the timed interval;
Swift reconstruction, AppKit drawing and authenticated SSH are excluded.

| Screen | Full API MB/s | Native delta MB/s | Reduction |
| --- | ---: | ---: | ---: |
| ASCII | 1.256 | 0.255 | 79.7% |
| Colored | 8.003 | 0.462 | 94.2% |
| Unicode stress | 51.508 | 1.738 | 96.6% |

One6s measurement per workload. Active PTY byte increments were about2500 bytes
per window in both variants. All destroy calls return0. Idle fetches a single
baseline only; its~0.007MB/s is not recurring idle traffic or an idle CPU repair.

Retained under `target/agent-validation/terminal-cpu-audit/`:
`probe-full-current.py`, `probe-native-delta.py`, `results-full-current.json`,
`results-native-delta.json` and generated snapshots. Tested Linux library SHA256
`2ffe7ce9c5b1efaf6c5649f9a558c39a2d44156566bccd3a3f9d21318f7a1dc8`.
This was the working-tree delta implementation before reducing cache retention
from full snapshots to selected-frame baselines; the wire protocol is unchanged.
CPU values are diagnostic shared-host observations, not physical-Mac budgets.

These results do not attribute the user's mostly-idle31.5%. Current-build native
idle baselines and actual high-CPU user-stack evidence remain separate gates.
