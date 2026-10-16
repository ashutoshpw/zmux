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

## Native red

At `e54e3bb512c630e7c006f8e43bf43828a871d760`, run `38853562903` builds
and passes the actual decoder checks and all 24 prior clipboard cases. The 25th,
`wrapped_word_first_row`, copies 100 cells rather than the full 108-character word.
Actual app5410/child5414 and all phase parent/nonce/kernel identities agree; native
focus is established. Native exit1 in7.613s, no timeout. All266 retained sources
match Git and recorded hashes. Archive SHA256:
`9374bad7d9f466803b1f8c29c0b9df0808dbeffaccc82ff28d026382749a8b91`.

The repair builds one displayed logical line from the same authoritative wrap
predicate as clipboard joining, then maps AppKit UTF-16 boundaries back to exact
row/cell endpoints. Word drag uses the existing common expansion path. No history
outside the viewport is read or guessed. Native green remains pending.

## Accepted native green and integration

Run `38854246653` at `6c58fb023e1f59d1d69e97b8b7778c1e03c7bb0e` passes
actual app build, production metadata decoder checks and all32 native clipboard
cases. This includes the unchanged intended regression, both word rows, forward/
reverse word drag, hard boundaries and exact decomposed combining-character words.
Actual app10045/child10104, every phase parent/nonce and kernel31x100 grid agree.
The native phase completes after shared shutdown in10.081s; all phases exit0,
without timeout or log truncation. All266 retained sources match Git and receipt
hashes; archive plist names the exact source, and the decoder's linked bridge
matches the archived app bridge.

Archive SHA256: `271ee3a90194bc4570296a38f8f719179dfc9f48036b3a652d34aea4a0f89f80`.
Bridge SHA256: `9ab80f65357f2094f5c256b96d234500570ab395458d411bc888e10c9f7c3c5a`.
Independent review: `/tmp/zmux-wrapped-word-green-independent-review.json`.

Integrated as `a6c8e3b` (red) and `a14d94d` (repair). All three terminal/fixture
source files are byte-identical to the accepted diagnostic commit. Only the plan
reference needed merge resolution, retaining both messaging and terminal entries.
Combined Rust owner checks at preceding97965e3 pass387 tests plus strict Clippy,
notices and both ABI checks; terminal changes alter no Rust/C input. Exact combined
main native acceptance remains separate. Physical mouse/VoiceOver, optional engine
wrap metadata, persistent anchors, autoscroll and full terminal parity remain open.
