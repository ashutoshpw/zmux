# T01 wrapped word selection

Extend double-click and word-drag selection across authoritative soft wraps in the
currently displayed viewport. AppKit remains the word-boundary owner; Rust engine
row metadata remains the only authority for logical continuation. Hard boundaries,
missing metadata and the viewport edges bound selection. Preserve cell spans and
UTF-16 mapping, including combining and wide cells. No guessed joins, retained
history anchors, optional-engine metadata or physical input parity claim.

The existing actual packaged TerminalView/PTY/clipboard fixture owns regression
coverage. Append a child phase containing one ASCII word longer than the actual
kernel width; double clicks on either physical row must copy the whole word.
The baseline stops at the physical row, a failure not exercised by the existing
24 cases (which cover character-copy joining only). Extend that same fixture for
forward/reverse word drag, hard boundaries and exact Unicode cell mapping rather
than introducing a parallel mock selection test. No production test-only seam.

Record intended native red before the production change. Run the unchanged
regression plus previous cases green, retain exact sources and real child/kernel
identity, then integrate only after source review and scoped checks. The existing
terminal-selection Actions suite provides this gate independently of the full
suite's outstanding Accessibility permission prerequisite.
