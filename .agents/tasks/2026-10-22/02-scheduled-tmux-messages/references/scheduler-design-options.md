# Scheduler screen design exploration

2026-10-23. Design artifacts only; no application change or new remote operation.

The supplied screenshot shows an empty striped table, equally weighted setup
buttons, raw health/version details and a visually flat message/time form. The
proposals keep Schedules as an embedded tab and retain existing capabilities:
Pending/History, relative/absolute display, source-bound target selection,
message and time editing, optional Return, cancellation, refresh and setup health.

## Options

### Desktop revision after user feedback

The user rejected the first proposals as web UI. Supersede those proposals with
three macOS desktop layout studies, all using the existing 1034 x 670 tabbed
client area. Use standard-size AppKit-style controls, system typography, native
table selection, separators and fixed window regions. No mobile reflow, page
cards, marketing headings or theme variations. HTML is a preview medium only;
the product remains Rust plus native AppKit.

1. Right inspector: horizontal split view, persistent modeless editing.
2. Attached sheet: full-width queue, edit/new sheet attached to the same window;
   deliberately modal while editing, with Cancel and default Save buttons.
3. Bottom details: full-width queue above a vertical split and modeless editor.

Keep setup information in a small status popover; expose refresh/new/edit using
standard toolbar controls. Preserve the source binding, relative/absolute input,
optional Return and the existing embedded Schedules tab. New shortcut choices
are outside this visual exploration. The revised artifacts are saved under
`/tmp/zmux-scheduler-desktop-designs/` and must be previewed at the screenshot's
actual size before presentation. Selection and implementation remain pending.

Revised studies passed bounded-window checks: 13 for each split layout and 17
for the attached sheet, including background modality, Cancel/Save, protected
drafts, history and time controls. Each 1034 x 670 export was visually inspected;
controls stay inside the client area. These remain illustrations, not AppKit
runtime verification. Exported PNGs and matching self-contained previews:
`01-right-inspector`, `02-attached-sheet`, `03-bottom-details` under
`/tmp/zmux-scheduler-desktop-designs/`. The previous artifacts are superseded.

### Superseded first proposals

1. **Native split**: dark queue/detail view, quieter connection status, readable
   message rows and a persistent inspector. Recommended for continuity with Zmux.
2. **Command desk**: compact full-width table with an editing drawer below. Best
   for scanning several pending messages and quickly editing without a narrow pane.
3. **Compose + agenda**: warm light surface, prominent composer and chronological
   queue. Best for scheduling one message quickly and reading what happens next.

Each self-contained HTML artifact uses sample data and local simulated controls.
Selecting rows, changing queue/history or time modes, editing, presets and saving
are design demonstrations; they do not call the Zmux bridge, SSH or tmux. Empty,
offline and setup details should also be inspectable. Example times are fixed
local-time samples rather than live scheduled deadlines.

Artifacts: `/tmp/zmux-scheduler-designs/01-native-split.html`,
`/tmp/zmux-scheduler-designs/02-command-desk.html`, and
`/tmp/zmux-scheduler-designs/03-compose-agenda.html`.

All three were visually previewed at 360, 728 and 1034 CSS pixels. Fifteen local
interaction checks per design passed, covering selection, editing, presets,
read-only history, invalid-time disabling, new-message creation, cancellation,
setup visibility, offline drafts, empty-state creation and horizontal overflow.
Final desktop previews have no console errors. Evidence:
`/tmp/zmux-scheduler-designs/validation.json`. The designs are presented as three
separate interactive artifacts in the thread. No commit, push or app implementation
is part of this exploration. Selection remains pending.
