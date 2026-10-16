# T01 native word and visible-row selection

## Contract and ownership

TERM-009 still lacks multi-click selection: `TerminalView.mouseDown` ignores
`clickCount`, and drag/copy operate on a simple pair of grid coordinates. Add
double-click word selection and triple-click physical visible-row selection,
including word/row drag extension, through one native selection owner. Keep
single-click character drag and the existing Copy/Select All entrypoints on that
same owner. This is original MIT implementation, not a translation of cmux.

Own `TerminalView.swift`, new `TerminalSelection.swift`, new real native selection
smoke and its isolated launch/runner route. Browser owner cleared disjoint startup
and smoke integration. Do not edit shared Rust, bridge ABI, models or browser and
metadata areas. Both vt100 and the optional engine already publish each run's
cell column/width and isolate non-ASCII cells; no new engine pin is needed.

## Public word policy

Use AppKit's public
[`NSTextView.selectionRange(forProposedRange:granularity:)`](https://developer.apple.com/documentation/appkit/nstextview/selectionrange(forproposedrange:granularity:))
with `.selectByWord` to obtain native word boundaries, then map UTF-16 positions
to the authoritative terminal cell spans. Apple's documented method adjusts
selection during mouse tracking. It avoids inventing a second terminal word
classifier. Record an actual named-Mac reference probe for punctuation, repeated
whitespace, underscore, slash, hyphen, equals, CJK and combining characters before
claiming their exact boundaries. Snap to complete wide/combining cells.

Triple-click selects the physical visible row. The current viewport does not
expose soft-wrap continuation metadata, so do not claim logical wrapped-line
joining, retained-history selection, autoscroll or copy-mode parity. Preserve
current snapshot/queue limits and existing geometry/history invalidation.

## Regression and implementation sequence

1. Freeze a regression first. Launch a real default-engine app with a private
   transient Desktop and an exact finite PTY child emitting fixed ASCII, styled
   and Unicode rows. Record actual public AppKit word-reference observations.
2. Deliver actual native double/triple-click events to the mounted TerminalView,
   invoke Copy through the real Command-C menu path, and compare independently
   specified clipboard literals. Baseline ignores click count and must leave the
   sentinel clipboard unchanged for the simple `alpha` word case.
3. Keep that exact case and command for the implemented owner, then require green
   character/word/row drag, punctuation/whitespace, Unicode/style-boundary and
   reverse-drag cases. Preserve and restore the hosted clipboard in memory.
4. Use bounded owned process execution, private socket, source/archive hashes,
   partial native reference/case receipts and actual shared shutdown. Keep the
   selection route separate from unrelated browser fixture failures.

Portable algorithm checks support the owner but cannot establish AppKit event
delivery, native word policy, clipboard routing, actual PTY or cleanup. No fake
TerminalView, injected selection outcome or test-only runtime seam is acceptable.
Idle run failures take priority. The parent owns main publication; separately
authorized diagnostic delivery preserves running workflows.

## Regression freeze preparation

The first regression asserts a public native outcome: double-clicking `alpha` in
the real PTY row `alpha beta`, then menu Command-C, copies exactly `alpha`.
The credible existing regression is ignored clickCount leaving a zero-length
selection and the clipboard sentinel. Existing special-key and scrollback journeys
do not exercise mouse multi-click selection. The new route is a private native
fixture entry like the engine/idle fixtures; it does not inject a runtime selection
result or fake the view. The future production helper will serve mouse drag/copy.

The registered macOS development workflow accepts explicit manual
`suite=terminal-selection` to run only the owned selection app journey, away from
browser gates. Pushes and default dispatch retain the full suite. Unknown manual
suite values fail before native work; selection/full concurrency groups differ.
It records actual AppKit word probes before the first literal
regression, partial report on failure, exact source/archive provenance, actual
child registration and successful shared shutdown only when all cases pass.
App45s/build600s/child120s bounds and the existing bounded process owner apply.
No baseline punctuation/whitespace expectation is claimed before that actual run.

Portable runner/child AST, YAML/shell syntax, source inputs and real owned Linux
PTY transport pass: the executable fixture accepts the actual login argument,
registers PID/PPID/nonce and emits exact ASCII/style/Unicode/ready rows. Receipt
`target/agent-validation/t01-selection-portable/receipt.json`, SHA256
`37230e9c18aa026fd36032d33428666b06f7dcc2c504dd5b0fd9e1e013f677ac`.
This does not establish native API behavior, input delivery or the intended red.

Before event delivery, the partial native report separately records actual key
window and TerminalView first-responder prerequisites. Missing focus is a setup
failure, never the intended no-selection red. Copy checks those prerequisites
again and retains the real menu route; existing menu validation permits Copy
with zero selection, but actual availability is still a native gate. The first
point is cell2 in row0: the bounds/grid center error is bounded below one quarter
cell at the required column count, so it remains inside `alpha`. Actual native
receipt and successful menu dispatch are required before accepting that red.
Cargo caches remain under repo `target/agent-validation/selection-<nonce>/cargo`.
The initial system reference list samples dot, repeated space, underscore, slash,
equals and CJK; hyphen, combining and emoji reference positions remain later cases.

## First native fixture admission

Run38830756527 at501cdca compiles the actual ARM host app, then stops at the
focus prerequisite: actual keyWindow=false and TerminalView firstResponder=true,
zero selection cases and no public word observations. All224 retained source
copies match that exact Git commit, and the native PID/PPID/nonce registration
matches its real PTY child. This is not the intended ignored-click-count red.
Artifact12016869221 ZIP SHA256
`99a2c867c5e8822ea0c188a359f7b8d4371310f7c91293895ac7b11363f1ea12`;
independent review `target/agent-validation/t01-selection-native-38830756527/independent-review.json`,
SHA256 `41bdc8cdb5c43acdee2b9e35f8c966c3902447923e27bb56f15ca8a13c94cde3`.

The first report did not retain app-active/window-visible state, so do not infer
their old values from a non-key window. Use the existing WindowLifecycleSmoke
native admission pattern: request makeKeyAndOrderFront and NSApp.activate for this
test-owned app, then yield MainActor for at most5s until actual active/visible/key
window and the mounted terminal responder are all observed. Record active,
visible and key state before/after that request. Preserve the real event/menu
route and literalalpha case. Public system word observations can be recorded
independently before a failed focus guard; clicks and Copy still require actual
focus. No forced activation, fake focus, changed production view or relaxed oracle
is part of this fixture repair.


## LaunchServices admission reassessment

Run38831911526 at1f3d492 again compiles the actual ARM app but executes zero
selection cases: active=false, visible=true, key=false, terminal responder=true
before/after the bounded native activation wait. All224 retained source copies
match Git1f3d492. Artifact12018405335 ZIP SHA256
`b0e657ece83e5e92924fd7ac1bb1e2ddbe9de5f776dbdfe4dcd477c07e7f9c95`;
source/observation review SHA256
`0d7724335c5c95b4724b868150011a925cfde52082bfc9165d9f30751ed12f18`.
The six actual public word probes selected alpha.beta, two spaces, value_name,
slash, equals and CJK respectively. These observations do not establish terminal
selection behavior, and the focus failure is not the intended regression red.

Reuse the original MIT browser-keyboard probe's exact-self LaunchServices
activation pattern: verify current PID, exact bundle URL and unique running bundle
identifier; disallow new instances and running-application substitution; require
returned PID to equal the directly owned process. Retain returned PID and any
activation failure in the partial report, then require the same observed
active/visible/key/responder conditions before unchanged literalalpha events and
menu Copy. The existing outer45s process-group bound still bounds the request.
No runner ownership, production TerminalView, event oracle or menu path changes.

Unexpected-new-instance arguments are `--cli --help`, not empty arguments:
AppStartup.prepare strips `--cli` and forwards `--help` to the bundled CLI before
AppKit/Rust/Desktop/storage startup; CLI help is offline. Thus an unexpected
instance cannot run the selection fixture, write its report or open a persistent
Desktop. Its returned PID fails admission and is never adopted or signalled.
Native activation remains a hosted gate until the exact frozen source executes.
