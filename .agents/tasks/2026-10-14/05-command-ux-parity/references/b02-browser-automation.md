# B02 native keyboard candidate (2026-10-15)

CMD-253/254/255 now have an original bounded local counterpart: browser press
(alias key), keydown and keyup, accepting one --key KEY or positional KEY plus
shared target options and --snapshot-after. The untrimmed token preserves literal
space, is limited to 128 UTF-8 bytes and rejects empty/NUL values. One shared
Keyboard wire action carries press/down/up; Rust denies read-only admission and
retains owner/document/deadline and submit-once/no-replay fencing. Frozen discovery
marks only local counterpart, with compatibility explicitly unverified.

Behavioral baseline is pinned cmux 3ba653b9e5afb11d0bc578031142950d6136697e,
its public docs/cli-contract.md browser keyboard row and the prior keyboard audit.
No implementation, tests or assets were imported or translated. The original
mapping uses public AppKit/CoreGraphics key events and standard ANSI virtual-key
positions: printable ASCII, Enter/Return, Tab, Escape, Backspace/Delete, arrow/
Home/End/Page keys, Space aliases, F1-F20, NumpadEnter and Shift/Control/Alt/Meta
left/right names (with Option/Command aliases). A literal uppercase or shifted
symbol keeps its explicit Unicode characters with currently held flags; it does
not implicitly hold Shift. Held Shift transforms ordinary letter/digit/punctuation
characters using the explicit ANSI mapping. Unimplemented known native names,
including lock/Insert/PrintScreen/Pause/ContextMenu/Clear/Fn, other Numpad keys and
F21-F35, fail native_key_unavailable. Full reference mapping parity remains open.

The exact BrowserView owns at most 64 held keys and serializes admitted keyboard
operations until their observation ends. A press prepares both native halves
before emitting either. Down/up preserve host-local modifier state; repeated down
is marked as repeat, and press on an already held code fails explicitly. All
prepared events pass a current owner/process/readiness and absolute deadline
check immediately before mutation begins; rejection sends no native event. All
native delivery goes straight to its WKWebView subclass without changing app/tab
selection, key window or first responder. A process-owned application subclass
contains tagged WebKit resends before app menus; tags remain recognizable for the
process lifetime with a bounded counter/nonce rather than expiring unsafe tags.
Ordinary user input is untouched. No global event posting, private WebKit API or
production activation is used. Owner/document/process replacement and close clear
held bookkeeping; no old-document key is replayed into a replacement.

Mapped native creation/delivery failure never falls back. Only opaque unknown
tokens retain the baseline page-event compatibility branch, with receipt
input:synthetic/native_defaults:false and isolated content world. It emits
synthetic keyboard phases without text/property/default compensation. Native
receipts identify input:native/content_world:native and establish dispatch, not
page default success or completed navigation. Snapshot-after observes the same
operation and cannot replay it. A dispatched observation timeout remains uncertain.

The standalone native experiment passed normal (non-debugger) run38802980621 at
0101763 on arm64/macOS15.7.9: all16 visible/detached cases, trusted key/default
behavior, unchanged unrelated responder/text, window-nil detached hosting, and
actual asynchronous Command-W resends intercepted in BOTH modes. Its report is
c96d5db85acb9566af7f488d10ae902924e7237300d89590dbbc1d8d52f9b588.
That is feasibility evidence only. The initial activation request could not take
focus; exact-self Launch Services activation corrected setup. An inherited actor
completion then trapped on launchservices.open-queue; LLDB38801277458 proved the
executor assertion and an explicit Sendable completion boundary repaired it.
Production keyboard commands never perform that harness activation.

Test authoring boundaries are distinct: the public CLI wire table protects literal
key bytes/aliases; actual socket policy denies read-only requests and refuses
portable fake native success; runtime owns admission bounds, normalization,
dispatched timeout uncertainty, navigation invalidation and no replay. The same
CLI regression first failed unknown subcommand press; runtime first failed unknown
Keyboard variant. Real DOM tests cover opaque untrusted events, unchanged values,
phase delivery and snapshot observations only. They do not fake native success.

BrowserKeyboardSmoke is authored through the actual packaged CLI on a never-selected
host. It requires native text/caret/Space/Enter defaults and trusted phases,
prevents a native default to ensure it is not forced/downgraded, checks literal
symbol semantics, separated down/up, exact cross-request ShiftDown/XDown/XUp/
ShiftUp/xDown/xUp ordering, real production async Command-W containment, explicit
opaque fallback, unavailable native-key failure, document reset and replacement
host with no transferred held state. It checks host/document identity plus exact
native responder and canonical selection retention. The packaged report now
requires browser_keyboard. Strict Swift6 and packaged native execution of this
production candidate remain pending. IME/physical input, inactive app delivery,
full mapping and modifier lifecycle acceptance are not claimed. Native callback-pool saturation after expired page promises is guarded before keyboard delivery and every downstream observation rejection preserves mutation uncertainty; its actual WebKit saturation journey remains an additional native gate.

Run 38822345401 at `924cd89e77a03e33b48381152183b29ba5aee53b` passed native
source typing/build, sidebar resizing, document-focus restoration and the existing
zoom/fill/click/interactions/edit-scroll journeys. It reached the production
keyboard fixture for the first time, failing its first native `press x` snapshot
title assertion. The original error did not record the returned title or DOM key
events, so asynchronous snapshot ordering and missing input cannot yet be
distinguished. The standalone feasibility probe waited before observing and does
not prove production snapshot ordering. Failure-only diagnostics now retain the
returned title, one subsequent actual DOM observation, app-active/key-window and
detached-host state. They preserve the strict assertion and add no delay, retry,
replay, synthetic event, default compensation or production behavior change.
Native keyboard acceptance remains pending; the separate diagnostic relaunch's
non-key-window sidebar drag failure is not the original failure.

# B02a browser automation bridge

## B02g type, select and pixel scroll (2026-10-15)

CMD-251/256/257 now have bounded local counterparts: `browser type`, `select`
and `scroll`. Type follows Fill's selector/text grammar, but text must be nonblank;
Select accepts one selector plus `--value VALUE` or one value operand. Both retain
raw CLI bytes until Rust admission, which bounds text to 8192 UTF-8 bytes, rejects
NUL/blank input, then trims whitespace/newlines. This follows the pinned server's
`v2String` semantics; Select's CLI can syntactically carry empty text but admission
rejects it. Fill still preserves literal whitespace and empty clearing. Its
CMD-252 discovery row is now reconciled as an existing local counterpart, backed
by its executable wire/policy/DOM checks; no compatibility status is promoted.

Behavioral references are cmux `3ba653b9e5afb11d0bc578031142950d6136697e`,
`CLI/cmux.swift:19593–19736`, `Sources/TerminalController.swift:8938–8973`,
`:9182–9265` and `Sources/TerminalControllerV2ParamParsingSupport.swift:5–9`.
No implementation, tests or assets were imported or translated. Type appends the
entire admitted chunk to the complete value/textContent, ignoring caret selection.
It uses ordinary DOM focus and cancelable `beforeinput` with insertText/data,
then the inherited native value setter plus input/change; non-value content gets
textContent plus input only. Cancellation fails input_rejected without forced
mutation. Value-bearing append captures its replacement before beforeinput, while
non-value textContent append observes page changes made by beforeinput. A real
DOM regression first failed the latter ordering and passed after the shared owner
was narrowed; the value-ordering counterpart passes throughout. No disabled/readOnly guard, per-character keyboard events, native
keyboard defaults, IME or trusted typing are implied.

Select requires any `value` property rather than a SELECT tag. It uses the inherited
setter, then input/change even when unchanged. It does not focus, dispatch click/
beforeinput, enforce disabled state or verify that an option matched; an unknown
option can leave the native value empty while setter dispatch succeeds.

Scroll accepts optional `--selector`, signed integer `--dx`/`--dy` and at most one
positional vertical delta (conflicting with --dy). Deltas default to zero and are
bounded to exact JavaScript-safe integers, magnitude at most 9007199254740991;
fractional/nonfinite/out-of-range values reject before admission. Omitted selector
uses instant window.scrollBy; explicit CSS first-match/exact ref uses element
scrollBy or scrollLeft/Top fallback. Actual geometry may clamp. No wheel events or
synthetic focus are added. All three share mutation-policy read-only denial,
submit-once transport, host/document/deadline fencing and optional bounded snapshot.
Successful values contain dispatched/ref, with null ref for window scroll.

Test authoring boundaries: CLI wire/grammar tests and actual socket policy protect
parsing, scope and honest native-unavailable failure; runtime tests independently
protect pre-dispatch normalization, byte/NUL/numeric bounds, once-only dispatch,
expiry uncertainty and document invalidation. Real DOM observers protect delivered
events, native setter/default behavior and actual offset/value changes. No test-only
production seam was added. The CLI regression failed unknown subcommand type; a
baseline-production control with a valid existing target returns unsupported_action
at the new append assertion. The production script passes 140 Chromium DOM cases,
including all prior 106 and 34 edit/scroll cases. Targeted browser suites pass
9 CLI and 16 runtime tests; the catalog suite passes 13 tests.

`BrowserEditScrollSmoke` is authored through the bundled CLI on a never-selected
real HTTP WebKit host. It verifies trimmed whole-value append despite a selected
range, first-match/ref reuse, cancelable edit without replay, beforeinput page
changes with distinct value/textContent ordering, selection events and
unmatched-option behavior, signed element/window offsets and optional snapshots.
It checks exact receipt host/document identity and canonical/native focus retention.
Failures carry actual observed values/events/document focus/ready state. The App
requires browser_edit_scroll. Exact-source strict Swift and actual packaged WebKit
execution remain pending; local DOM or portable Rust checks do not establish them.
Native press/keydown/keyup remain a separate unimplemented keyboard group.

Final isolated verification archived committed baseline `6e63dbe` and overlaid
only 15 owned browser files/blocks, excluding R03 WIP. Full scripts/check passes
272 Rust tests across 50 suite outputs, formatting, strict all-target workspace
Clippy, dependency notices and C ABI. The exact frozen source passes 140 real
Chromium cases and 69 catalog probes for 428 frozen rows, compatibility false.
The non-value beforeinput ordering regression has recorded red and green results;
its value-bearing counterpart protects the distinct pre-event capture behavior.
Source/log/receipt artifacts are retained at
`/tmp/zmux-b02g-edit-scroll-isolated-20261015/`. No staging, commit or push was
performed by this author. Parent owns independent review, integration and native
execution; local Rust/Chromium evidence does not close those gates.

## B02f browser interaction family (2026-10-15)

CMD-245–250 now have bounded local counterparts: `browser dblclick`, `hover`,
`focus`, `check`, `uncheck` and `scroll-into-view`; `scrollintoview` and `scrollinto`
alias the last command. All accept `[--selector SELECTOR | SELECTOR]`, named target
options and optional `--snapshot-after`. CSS chooses the first match, while refs
retain their exact host/document identity. Selector byte/NUL bounds and single
selector grammar apply before admission. Discovery marks only these six frozen
rows as local counterparts with historical compatibility unverified.

The behavioral audit used pinned cmux
`3ba653b9e5afb11d0bc578031142950d6136697e`, `Sources/TerminalController.swift`
near 8820–8870, 9164–9182 and 9269–9282. No reference implementation, tests or
assets were imported or translated. The independent Zmux implementation retains
Click's existing wire variant and shares original event-phase, activation and
inherited native-property helpers with the new supported-operation enum. Fill
uses the same native-property setter without changing its event contract.

Dblclick rejects native disabled targets, scrolls nearest, performs two complete
click attempts, then dispatches one synthetic `dblclick` with detail 2. Each
attempt preserves Click's over/enter/move/down/focus/up/activation ordering and
cancellation behavior. Hover scrolls nearest and emits pointer/mouse over, enter
and move only; disabled targets are allowed and it does not focus or activate.
Synthetic events do not establish trusted OS input or guarantee CSS `:hover`.
Focus calls ordinary DOM `focus()` if available, preserving default scrolling;
there is no disabled guard and completion does not promise a focusable element.
It remains separate from native app/tab focus.

Check/uncheck first require a `checked` property, then reject native disabled
controls. They scroll nearest and focus with preventScroll before inspecting the
state. An already satisfied state emits no activation/input/change. A change uses
one DOM activation for checkboxes and radio check; radio uncheck instead uses the
inherited native checked setter and input/change without click. Every path
verifies the final strict Boolean state. A canceled activation that leaves the
wrong state fails `not_changed`; it is not forced or replayed. Scroll-into-view
uses center/center/instant scrolling without synthetic focus or activation.

All six are mutation-policy operations: focus/scroll handlers can execute page
code, so read-only sockets deny admission. They share exact owner/document and
absolute-deadline fencing, physical callback capacity, optional bounded snapshot
and submit-once transport. They are not explicit navigation requests. A dispatched
navigation/deadline failure can be uncertain and is never retried automatically.
Successful values report `{dispatched:true, ref}`; they do not promise a successful
default action, a trusted gesture or page readiness. App selection and native
key/responder ownership are retained.

Test authoring boundaries: public CLI capture protects grammar/aliases and literal
wire payloads; a real control socket protects read-only denial and honest
native-unavailable failure. Runtime tests protect selector admission, queued
versus dispatched timeout uncertainty and document changes that must invalidate,
rather than promote/replay, an interaction receipt. DOM observers independently
check delivered events, native default actions and resulting checked/focus/scroll
state. No test-only production seam was added. The initial CLI wire regression
failed with `unknown browser subcommand dblclick`; the actual production-script
DOM suite failed its double-click dispatch assertion before implementation.

The real Chromium suite passes 106 cases (62 prior plus 44 family cases), including
two complete activations/detail 2, checkbox double-toggle, disabled hover,
ordinary focus scrolling, first CSS/ref reuse, no-op checked state, native radio
group changes, inherited radio uncheck setter, canceled check/uncheck with
`not_changed`, center positioning and invalid/missing/disconnected targets. The
scroll assertion uses the document's actual scrollport dimensions, excluding
scrollbar gutters. Portable browser suites pass 9 CLI and 15 runtime tests.

`BrowserInteractionSmoke` is authored against a never-selected real HTTP WebKit
host through the bundled CLI. It checks first-match double activation/detail 2,
two checkbox defaults, disabled hover, DOM focus scrolling, checked/ref/no-op
state, radio uncheck, canceled check without replay, both center-scroll aliases,
optional snapshots and selector/ref failures. Every successful mutation verifies
actual receipt host/document identity and canonical model/native focus retention.
The App runs it after Click and requires `browser_interactions`. Exact-source
strict Swift compilation, actual packaged WebKit execution and physical keyboard/
pointer behavior remain separate hosted gates; this Linux evidence claims none.

Isolated verification archived committed baseline `2360fb0` and overlaid only the
15 browser-owned files/blocks, excluding concurrent R03 and surface-order work.
Full `scripts/check` passes 264 Rust tests across 49 suite outputs, formatting,
strict workspace Clippy, dependency notices and C ABI. The same frozen source
passes all 106 Chromium DOM cases and `commands --check` passes 65 parser probes
for 428 frozen rows with compatibility false. Logs and source manifest are under
`/tmp/zmux-b02f-interactions-isolated-20261015/`. No staging/commit/push was performed
by this author; parent owns independent review, integration and native evidence.

The first opaque strict-click sequence failure remains individually unresolved:
its log contains no observed events/result. A later diagnostic-only run reproduced
the same assertion with `dispatched: true`, the correct first target, every
synthetic pointer/mouse/activation event in order, and only the native `focus`
event missing. The document was visible but still loading and `hasFocus: false`
before/after; `activeElement` correctly changed to the clicked button. This
reproduction establishes a startup/document-focus precondition race for the new
failure, without retrospectively proving the opaque failure had identical data.
Raw diagnostics are retained in the v2 snapshot; no automation fix was justified.

The DOM harness now waits at most two seconds for `readyState == complete` and
actual document focus, asking the window to focus and failing with context if
that precondition cannot be established. Its original 20-second subprocess bound
and every one of the 106 assertion predicates remain unchanged. Click failures
retain the returned envelope, observed events, other-target events and before/
after focus/loading/visibility/scroll state. Failure rendering uses textContent so
HTML-like observed text is preserved. The exact diagnostic/precondition snapshot
passes all 106 cases once at
`/tmp/zmux-b02f-interactions-isolated-v3-20261015/dom.log`.

Native Click and Interaction fixtures already exclude native `focus` events from
their strict event lists; their DOM-activeElement assertions and native responder
retention checks remain intact. They now retain observed event/state/focus JSON
on failure, including document focus/loading/visibility. Their hidden-host checks
were not given a foreground-focus precondition. Automation production and Rust/
CLI bytes are unchanged from the validated interaction freeze. The 16-file v3
freeze adds only runner preconditions/diagnostics, native fixture diagnostics and
this evidence note; exact-source Swift/native execution still awaits hosting.

## B02e click candidate (2026-10-15)

Frozen CMD-244 has a bounded local counterpart: `browser click [--selector SELECTOR |
SELECTOR] [TARGET_OPTIONS] [--snapshot-after]`. The behavior audit used cmux
`3ba653b9e5afb11d0bc578031142950d6136697e`, `CLI/cmux.swift` around 19500–19548 and
`Sources/TerminalController.swift` around 8805–8820/8896–8918. No reference
implementation, tests or assets were imported or translated.

CSS chooses its first match; a document ref addresses its exact element. Selectors
retain the 2048 UTF-8 byte/NUL bounds. A native disabled property rejects before
scroll/event dispatch. The fixed isolated-world script scrolls nearest, sends
pointer/mouse over/enter/move/down, focuses the DOM element with preventScroll,
sends up, then invokes exactly one DOM click activation. Non-HTMLElement targets
without a click method receive one MouseEvent fallback. Enter events do not bubble;
mouse down/up carry detail 1. Pointer/mouse events are synthetic and this is not a
trusted OS gesture or a claim of keyboard/pointer injection into the native app.

Native DOM activation owns checkbox/link/default behavior and cancellation. As in
the audited reference, canceling down events does not suppress the later click;
canceling the click itself can roll back a checkbox default action. A successful
value contains `dispatched: true` and a reusable ref. It establishes dispatch,
without promising that a page allowed its default action or that navigation has
finished. Snapshot-after reads the bounded snapshot after the same activation;
it cannot repeat the mutation if observation fails.

Click uses the existing mutation admission and physical callback owner. Read-only
clients deny it. Epoch/host/document and absolute deadline fencing apply; a page
handler can run arbitrary work or navigate, so a dispatched failure can have an
unknown outcome and is never replayed. Click is not classified as an explicit
navigation request: navigation can complete after its dispatch receipt, or replace
the document before callback completion and produce an uncertain failure. Callers
must inspect/wait for the resulting page rather than blindly retrying the click.
No modifiers, multiple clicks, check/type/press, frames, shadow traversal, trusted
input or complete B02/F01/V01 parity are claimed. CMD-244 discovery maps to a
local counterpart with compatibility explicitly unverified; no behavioral parity
status is promoted. Other historical rows are unchanged.

The same CLI wire regression first failed with `unknown browser subcommand click`,
then passed after implementation. Targeted local browser suites pass 9 CLI and 14
runtime tests. Existing mutation deadline coverage now includes click: queued
expiry is known unexecuted, dispatched expiry is uncertain, and drain never
replays it. Selector bounds and real socket read-policy/native-owner failures are
covered at their production owners.

The production-owned script passes 62 real Chromium DOM cases (previous 48 plus
14 click cases); the added cases failed on the pre-click script. Independent DOM
observers check the pointer/mouse/focus sequence, synthetic event identity,
enter bubbling/button/detail state, nearest scroll/center coordinates, first CSS
match and exact refs, one activation plus snapshot, native checkbox activation,
down-event cancellation, click default rollback, disabled rejection, SVG fallback
and missing/invalid/disconnected targets. This is Chromium DOM evidence, without
WebKit, isolated-world or native lifecycle claims.

`BrowserClickSmoke` is authored through the bundled CLI against a never-selected
HTTP WebKit host. It checks actual page events/default actions/cancellation and
receipt identity. Its link navigates to the real second fixture page, counts one
source click through sessionStorage, accepts only a dispatch completion or an
explicit no-retry uncertain navigation/deadline error, waits for actual native
and published page readiness, and rejects the old document ref. The fixture also
requires native key/responder and every model selection to remain unchanged.
The App calls it after fill and before ordering/download fixtures; packaged
acceptance requires `browser_click`. Exact-source strict Swift compilation and
native execution remain required; no native pass is established for this slice.

## B02d fill candidate (2026-10-15)

Frozen CMD-252 has a local counterpart: `browser fill [--selector SELECTOR | SELECTOR]
[--text TEXT | TEXT ...] [TARGET_OPTIONS] [--snapshot-after]`. Explicit text wins
when positional text is also present; positional words join with spaces. Omitted
or empty text clears. Text must fit 8192 UTF-8 bytes without NUL; selectors retain
the existing 2048-byte bound, targeting and document-ref ownership rules.

The behavior audit read the reference CLI fill grammar and selector-action owner:
CSS selects its first match, while a document ref addresses its exact node. Existing
find/wait ambiguity rules are unchanged. The fixed isolated-world WebKit script
focuses the DOM element, dispatches cancelable `beforeinput` with replacement text,
then uses an inherited value setter (or plain textContent for non-value elements)
and emits input/change. A canceled beforeinput reports `input_rejected` without
forcing a replacement or emitting input/change. Value-bearing elements receive
change; non-value text replacements receive input. Replacement text is never HTML.
No reference implementation code was imported.

These are synthetic DOM events, not trusted keyboard input. Browser fill is a
mutation denied to read-only clients. It uses existing once-only admission,
physical callback credit, target/epoch/host/document fencing and absolute deadlines.
Any dispatched fill can invoke arbitrary page event handlers; uncertainty never
causes a replay. The successful value contains `filled` and a reusable document
ref, without echoing replacement text. Optional snapshot-after reads a bounded
snapshot after the same mutation; a snapshot error does not replay the edit.
Frames, shadow traversal, click/type/press and full B02/F01/V01 parity stay open.
The frozen compatibility rows remain unverified.

The CLI wire test first failed with `unknown browser subcommand fill`, then passed
with the parser/runtime implementation. Local targeted suites pass 9 CLI and 14
runtime browser tests, covering empty/exact text transport, bounds/NUL, read policy,
missing native owner, once-only dispatch and expired mutation uncertainty.
`scripts/check-browser-dom` passes 48 real Chromium cases (the previous 35 plus
13 fill cases). The added cases failed against the committed pre-fill script and
pass against the production-owned script, observing first-match mutation, native
setter behavior, page event order/cancellation, empty clearing, Unicode/multiline,
contenteditable/plain text, one-mutation snapshot, missing/invalid/disconnected refs.
This is real DOM evidence, without WebKit or isolated-world execution claims.

`BrowserFillSmoke` is authored through the bundled CLI and a never-selected native
WebKit host. It checks actual page values/events, first-match/ref reuse, canceled
input, empty/Unicode/plain-text replacement, one snapshot, exact receipts and
native key/responder plus all model selection retention. The focus baseline is
captured after fixture creation/setup so this gate measures fill itself. The App
calls it after zoom and before the separately gated download fixture; packaged
acceptance requires `browser_fill`. Exact-source strict Swift compilation and
native execution remain required; neither has passed for this candidate.

## B02c live page zoom candidate (2026-10-15)

Frozen CMD-232–235 now have a local implementation: `browser zoom in|out|reset|FACTOR`
with the existing target options, including the hard window constraint. One exact
runtime owner admits the mutation, dispatches it once and fences its receipt with
the existing epoch/host/document identity. Native menus call the same admitted
action. Public `WKWebView.pageZoom` owns the actual state; no page JavaScript,
private WebKit API or document-navigation counter is used for zoom.

Explicit factors must be finite; native execution clamps them to 0.25–5.0,
including negative/zero and oversized finite values. In/out change by 0.1 and
clamp at those limits; reset selects 1. Reset to a configured default remains
unresolved until settings support exists. Completion reports the actual native factor.
Zoom is a mutation denied by read-only control access. Hidden targets retain tab,
pane/workspace/window selection and document generation. The shared deadline and
no-replay rules apply; a timed-out dispatched mutation has an unknown outcome.
This is live-host state, without default-setting persistence, host-recreation
restore or viewport/device emulation. BA-10 and full B02/V01 remain incomplete;
historical compatibility rows and the F01 unverified boundary are unchanged. Four
executable discovery leaves describe zoom grammar, bounds and native requirements;
`commands --check` parses their probes without executing native mutations.

The CLI wire regression first failed on the old parser with `unknown browser
subcommand zoom`, then passed after implementation. Scoped Linux checks pass all
9 CLI browser tests and 13 runtime browser tests, including finite boundary operands and NaN/infinity,
read policy, missing native owner, once-only dispatch, deadline uncertainty and
late-result rejection. These tests do not claim native zoom execution.

A reference behavior review found the old local menu limits were narrower than
the audited command: the reference accepts any finite factor and clamps to
0.25–5.0. The existing CLI wire table was extended for 0.25, 5, negative/oversized
finite factors and uppercase directions; it failed against the draft's range
rejection and passes after repair. Native clamp checks also include zero and
an extreme finite factor. No reference implementation code was imported.

`BrowserZoomSmoke` is authored against the bundled CLI and actual WebKit hosts.
It compares each receipt with `WKWebView.pageZoom`, checks explicit factors,
menu-shared admission, step clamping, reset, invalid nonmutation and a never-selected
hidden host while preserving the original host/document/selection. The App smoke
calls this before the separately gated download fixture. Strict Swift compilation
and exact-source packaged macOS execution remain required; no native pass is yet
established for this candidate.

## Scope

Implement bounded asynchronous browser requests and completion receipts between the Rust
desktop executor and trusted native WebKit host. Initial commands: back/forward/reload,
eval, DOM snapshot and get/predicate reads. This does not close all B01/B02 workflows.

Targets use existing caller/UUID/ref/index routing and authorization. Read-only clients
may use built-in reads; eval/navigation mutate and remote relay stays denied. The local
socket listener must never wait for WebKit. CLI admission/result polling is bounded.

Bind operations to identity namespace, surface, native host incarnation and document
generation. Native navigation advances independently of Rust's navigation-intent counter.
Navigation invalidates stale DOM refs and unrelated pending requests; only the causal
navigation operation follows its own transition to completion. Closed/replaced hosts
cannot complete work on a newly mounted surface. Hidden surfaces require no focus change.

Built-in reads use an isolated content world. User eval reports its actual world and is
never replayed automatically. No page gets a native message handler or socket capability.
Bound script input, queued operations, output bytes, snapshot nodes and deadlines.

## Ownership

- Runtime/target/policy lifecycle: persistence_io agent.
- CLI parsing, bounded result polling and help: persistence_cli agent.
- BrowserAutomation/BrowserView WebKit owner: session_runtime_review agent.
- Native ABI, App routing, native transport and public-path smoke: parent.

## Acceptance

Rust owner tests establish cancellation, generation and authorization invariants. CLI
tests establish grammar/transport/polling. Packaged macOS smoke must use the actual
bundled CLI against a local two-page fixture for navigation completion, reads/eval,
snapshots/refs, hidden targets, stale-document rejection and terminal responsiveness.
Source checks or direct WebKit calls alone do not prove the public automation path.

Profiles/downloads, semantic input, locators/waits/frames, cookies/storage/state, developer
workflows and full audited options remain open until implemented and verified.

## Current implementation and evidence

- Runtime, CLI and native bridge implemented; parent scripts/check passes 136 tests,
  strict Clippy, format, notices and foreign C ABI.
- Rust owner tests cover host/document/restore fencing, once-only dispatch, bounded
  pending/results/history, late navigation after timeout, and forged native event rejection.
- CLI tests cover actual portable-runtime timeout/unavailable/policy outcomes without a
  fake successful DOM provider. The listener remains available while work is pending.
- Original JS DOM harness: 14 real Chromium cases passed, independently rerun by parent.
  This is not WebKit or the native command path.
- Native host bounds physical pending JavaScript callbacks even after logical timeout,
  so repeated never-resolving Promise evaluations cannot grow without limit.
- Public CLI macOS fixture authored; hosted compile and native smoke remain pending.
- Fresh gpt-6.1-sol reviews accepted repairs: shutdown cancels outstanding receipts;
  hidden hosts receive a bounded viewport; generation-tagged native event queues recover
  transport loss through full-owner reattachment and polling wakeups. Protocol regression
  verifies reattachment invalidates stale callbacks and cannot hijack another host.
- Native failure assertions now require exact error categories and the responsiveness
  check observes eval's page-title side effect before sending terminal input.
- macOS compilation and WebKit smoke remain pending. Never-selected hidden browser
  viewport behavior needs its own native journey when no-focus creation is added.

## Browser followup (in progress)

- `8ee0ee3` universal native compilation passed, but packaged smoke failed its pending
  eval error-category assertion. The assertion originally omitted the actual error.
  WebKit can collect an unreferenced never-settling Promise; the fixture now roots its
  Promise and includes bounded failure output. The exact hosted cause remains unproven
  until the next run; production errors are not reclassified to satisfy the check.
- Foundation macOS exposed a temporary-directory collision between concurrent tests
  sharing a clock tick. Fixtures now include a process-local atomic sequence.
- No-focus browser creation preserves the selected workspace, pane and tab and returns
  the created identity; native acceptance exercises a never-selected host viewport.
- CSP handling probes a fixed literal before user code. Only a recognized policy denial
  selects isolated execution; the user script is never replayed on its own exception.
  Six Chromium cases passed; actual WebKit diagnostics remain a hosted gate.
- Parent full `scripts/check` passed after these changes. Fresh reviews and hosted
  execution are pending. B01, B02 and full V01 remain open.

The undefined compatibility marker retains the observed wire spelling, independently
implemented. DOMRect and Promise results are normalized without copying upstream code.

## Receipt observation repair

- Hosted `c9fb697` failed the portable pending-eval check on Darwin: socket receipt
  read exhausted the client's deadline. A real-runtime forwarding regression with
  300 ms admission delay reproduced the failure locally before the fix.
- CLI now reserves a bounded proportional window for admission and receipt observation
  within the existing timeout. The regression passes; a stalled-status negative control
  returns uncertainty, never fabricates a runtime timeout, and proves one admission.
- Parent full checks and fresh narrow review pass. Hosted validation remains pending.
  The shared socket transport still has per-I/O timeout semantics; trickled replies
  require a separate absolute-deadline repair before claiming a hard wall-clock bound.

## Hosted followup evidence

At `50309f9`, Foundation checks (`38731671194`) pass and macOS development run
`38731671218` passed universal compilation and packaged browser smoke. This includes
never-selected viewport reads, CSP isolated fallback and no replay after a CSP-shaped
user exception, and terminal input while a rooted evaluation remains pending.
B02 advanced browser commands and full V01 remain open; this artifact is not the
requested full-parity handoff.

## Browser title observation gap found in native artifact

At c9db69c, the AppKit header/tab retained Second fixture while its address was /csp
and the separate WebKit PNG showed CSP fixture. Header/tab both refresh from the Rust
surface title, ruling out separate stale drawing caches. BrowserView publishes title
only in didFinish; later title changes are not observed. Dynamic document.title changes
therefore have no owner update. The exact late-title ordering of the CSP load is not
instrumented and remains unproven. Repair should observe title changes with bounded,
lifecycle-fenced native publication and verify model/header/tab against final WK title;
an arbitrary capture sleep would not repair the missing observation owner. No fix yet.

## Live title and observation repair

A real executor regression reproduced stale unfenced `ObserveBrowser` updates
overwriting a newer CLI navigation. Native observations now carry exact epoch,
identity namespace, host, document generation and model navigation; validation
and shared model application occur on the workspace executor. Generic action
ingress cannot bypass this owner. Raw title/URL bounds remain 4096/8192 bytes.
The focused regression is green and parent full checks pass (181 Rust tests,
formatting, strict Clippy, notices and C ABI).

The native regression changes a live page title and waits for the Rust model,
window header and tab to agree. The initial repair routes load-completion
observations through the trusted channel; its missing live-title observation
is deliberately retained for the hosted regression. A subsequent bounded KVO
consumer will publish current WebKit state without one Task per page mutation.
Native compilation and this title regression are pending hosted execution.

Hosted red established at `3a9ab8e`: macOS38745262578 compiled universally and
failed precisely with “Browser document title did not reach the Rust surface:
Changed title fixture”. The corrected notification-reply journey completed before
this failure. Foundation38745262234 passed.

The live-title repair registers KVO with a Sendable callback that yields only a
void signal into one newest-value slot. One MainActor consumer reads current
WebKit title/URL and current owner; an old signal carries no stale page data.
Navigation, close and process termination invalidate observation; transport recovery
reannounces the latest current page. Fresh source review found no blocker.
Hosted green on the repaired source remains pending.

## B02b frozen readiness and locator contract

This slice implements main-document discovery/readiness, not pointer/keyboard input,
profiles, frames, shadow-root traversal or a complete platform accessibility tree.
It is original MIT code; no cmux implementation or tests are imported.

Public grammar (TARGET_OPTIONS may precede the browser verb or follow operands):

```text
browser find role ROLE [--name TEXT] [--exact]
browser find text|label|placeholder|alt|title|testid TEXT [--exact]
browser find first|last --selector CSS_OR_REF
browser find nth --index N --selector CSS_OR_REF
browser wait --selector CSS_OR_REF [--state STATE] [--timeout-ms N]
browser is visible|enabled|checked CSS_OR_REF
```

Canonical `is` and `wait --selector ... --timeout-ms ...` follow the public cmux
CLI contract (`docs/cli-contract.md`, browser table) and its published browser
command reference (`skills/cmux-browser/references/commands.md`, Wait). The find
families/role-name-exact and first/last/nth selector/index operands follow this
task's frozen browser audit. Convenience aliases `get visible|enabled|checked`
and positional `wait CSS_OR_REF` are additive; they do not replace canonical
commands. No upstream implementation is read or translated for this slice.

TARGET_OPTIONS remain `--workspace`, `--pane`, `--surface`/`--tab`,
`--expect-revision`. `--` permits literal option-looking text. Repeated, conflicting
or irrelevant options fail before admission. `--name` is role-only; `--exact` is
semantic-locator-only; `--index` is nth-only and is zero-based, 0–4095. First/last/nth
require `--selector` and no text. Wait `--timeout-ms` accepts 1–30000 milliseconds
as the execution budget, with at most one second of bounded CLI receipt headroom.
An explicitly supplied global `--timeout` remains an outer admission/result
deadline and may shorten that wait; without it, `--timeout-ms` sets the CLI budget
plus headroom. Without either flag, the existing CLI default remains. Native and
runtime deadlines derive from the admitted budget, never independent fresh clocks.

Semantic queries normalize whitespace (`trim`, collapse Unicode whitespace).
Default comparison is case-insensitive substring; `--exact` means case-sensitive
normalized equality. Roles are compared as lowercase exact tokens. Explicit role
uses its first nonempty token; implicit roles cover links, buttons, headings,
images, lists/items, text inputs/textarea, checkbox/radio/range/number inputs,
select/option and progress. Role names resolve, in order, through aria-labelledby,
aria-label, associated HTML labels, image alt, button-input value, button/link text,
then title. Label queries match associated label text (including aria labels),
placeholder/alt/title/testid query those attributes (`data-testid` for testid).
Text queries match innermost matching content elements, excluding script/style/
noscript/template. Discovery includes hidden DOM elements; readiness is explicit.
Exactly one semantic match is required (`element_not_found` or `ambiguous_locator`
otherwise). First/last/nth explicitly select document order from CSS/ref matches.
Malformed CSS fails `invalid_selector`; selection beyond results fails
`element_not_found`. No locator silently searches another frame or document.

Find returns the existing result envelope with `value: {ref, role, name, text}`;
descriptor name/text are bounded like snapshots. Refs remain scoped to exact host
and document generation and are reusable by get/wait. Stale or disconnected refs
always fail `stale_reference`, including negative waits; they never become CSS.

Wait states are `attached`, `detached`, `visible` (default), `hidden`, `enabled`,
`disabled`, `checked`, `unchecked`. CSS/ref waits require at most one matching node;
ambiguity fails immediately. Missing CSS matches satisfy only detached/hidden.
Visibility uses the existing computed-style/nonzero-box predicate; enabled uses
native disabled plus aria-disabled; checked uses checked plus aria-checked.
Success returns `value: {matched: true, state, ref}` with null ref when absent.
Only get/find/wait/snapshot are read actions; existing eval/navigation policy stays.

Bounds remain selectors/query text 2048 UTF-8 bytes, role 128 bytes, reference
registry 4096 and result 64 KiB. Locator enumeration visits at most 4096 elements, 16384 total DOM nodes and
1048576 UTF-16 text code units, including excluded content,
and fails `dom_scan_limit` if incomplete rather than returning a partial match.
Waits use short isolated-world probes separated by a cancellable 100 ms native
timer. One operation owns at most one physical JavaScript callback, sharing the
existing cap of eight with eval/reads. Timeout never releases a physical callback
until WebKit returns; no page Promise or unbounded page polling loop is installed.
An absolute native deadline is checked before dispatch and completion. Navigation,
host/namespace change, close and timeout terminate the exact pending owner; a
later document cannot satisfy its wait. Hidden views stay hidden and focus remains.

Acceptance owns three distinct risks: runtime/CLI tests protect read policy,
validation, lifecycle/deadline and submit-once transport; a real DOM harness checks
locator semantics independently; packaged WebKit uses the actual bundled CLI to
find/wait/reuse refs on delayed and hidden DOM, reject ambiguity/stale refs, time
out and cancel on navigation while the terminal stays responsive. Linux checks do
not establish native compilation or WebKit success.

## B02b implementation and local evidence

The runtime/CLI/native implementation now includes the frozen find/is/wait grammar,
read-only policy, exact document references and bounded native timer probes. Native
callbacks keep their physical slots after timeout, with deadline checks before dispatch
and completion. Text discovery excludes script/style/noscript/template descendants;
role names are deliberately the documented common HTML subset.

Focused checks use `CARGO_TARGET_DIR=/tmp/zmux-b02b-target` to prevent another checkout's
concurrent Cargo build from replacing the CLI executable between build and invocation.
`cargo test -p zmux -p zmux-desktop-runtime --test browser` passes 18 tests (6 CLI,
12 runtime); strict Clippy for those packages/all targets and workspace formatting pass.
The legacy leading-target URL regression first failed privately with “expected pane
and URL”; the repaired adapter passes for open/navigate/goto. Earlier shared-target
runs are not the evidence for this regression.

`scripts/check-browser-dom` passes 35 real Chromium DOM cases using the actual Swift-owned
script. It checks semantic naming, Unicode whitespace, hidden discovery, strict ambiguity,
ordered selection, reusable/stale refs, readiness transitions and explicit element/node/text
scan-limit failures. It is independent DOM evidence, not isolated-world/WebKit proof.

The packaged App smoke now uses the bundled CLI against a never-selected WebKit host for
role/name/exact discovery, label/testid agreement, ref reuse, delayed/hidden readiness,
strict ambiguity, timeout and disconnected refs; focus retention is asserted afterward.
Swift compilation and this new smoke await hosted macOS execution. Native concurrent
wait/navigation ordering and terminal input during an admitted wait are not established
by this fixture; runtime owner tests cover cancellation/late completion, and the existing
native pending-eval journey separately covers terminal responsiveness. Physical callback
saturation and native scheduler-delayed deadline behavior remain hosted verification gaps.
No pointer/keyboard, frame, profile or full B02/V01 parity claim is made.

Parent integrated validation passes 193 Rust tests, strict Clippy, formatting, notices and
the C ABI in a private build directory. The parent also ran all35 Chromium DOM cases.
Linux Foundation CI now executes this DOM suite; packaged native acceptance requires
`browser_locators_waits`. Hosted results remain pending.

Hosted receipt: Browser live-title observation repair executed successfully at5f5005e in
macOS38753451286 (universal build and packaged smoke). Parent downloaded and checked
archive/source/required flags. This closes that subset native gate only; full package
parity and applicable physical/external-host gates remain open.

## DOM harness ownership and timeout diagnostics

Foundation `38759082378` timed out its Chrome subprocess after 20 seconds without
preserving browser diagnostics. Subsequent hosted `38760632422` passed all 35 cases;
its runner manifest reports Chrome 155.0.8059.39. The original CI stall is not diagnosed
as a locator defect or a particular browser bug.

Local Chrome 151.0.7922.108 reproduces a pre-fixture stall: the browser opens TCP
connections to the loopback port, but the server receives no HTTP GET before timeout.
Unique profile, load gating and virtual-clock removal did not reliably repair it;
those experiments are not claimed as the hosted Chrome 155 root cause.

The harness now prints its exact browser version, creates a private profile for each
invocation, and owns its browser process group. The original 20-second deadline and
all 35 DOM assertions remain. `scripts/browser_process.py` independently drains both
pipes while retaining at most a 1 MiB stdout prefix and 64 KiB stderr tail; output
beyond either cap is counted and drained, and truncated DOM output is rejected.
Timeout kills only that invocation's group, allows at most one second to drain pipes,
and bounds leader reaping to three seconds. Successful leader exit also stops owned
descendants, including those that have already closed both output streams. Diagnostics
report the browser version, bounded retained output, total byte counts and fixture GETs.

The existing terminal-feasibility lifecycle fixture now also calls the real browser
helper through a result adapter. All nine real-process tests pass: both runners cover
early EOF, successful-leader descendants, exited leaders whose children hold pipes and
2 MiB stdout floods; a simultaneous stdout/stderr flood separately checks the browser's
independent prefix/tail bounds and complete byte counts. An in-memory control retaining
the old unbounded capture and missing success-path cleanup fails specifically on a live
descendant and the oversized retained output. No fake browser supplies DOM success.

Fresh Chromium 155.0.8059.79 passes all 35 cases with this helper. An intentionally
wrong accessible name fails through the real DOM assertion; an intentionally blocked
real renderer receives one fixture GET, hits the unchanged deadline and completes owned
cleanup in 20.93 seconds. The earlier diagnostic
iteration also passed two simultaneous invocations and reported local Chrome 151's
version and zero fixture GETs on its timeout; those results do not diagnose hosted
Chrome 155. Fresh hosted execution remains a separate delivery gate.

Final integrated portable verification: `scripts/check` passes in fresh snapshot
`/tmp/zmux-move-zoom-integrated-jfol05gu` (255 Rust tests, formatting, strict
Clippy, dependency notices and C ABI). All 35 captured production/native/script
inputs match the working source after verification; log
`/tmp/zmux-move-zoom-integrated-check.log`. Swift compilation and actual native
execution remain separate hosted gates. The preceding `16cf61a` universal app
compiled but its window close fixture timed out; new bounded phase traces retain
evidence without another app launch or extending the timeout.
