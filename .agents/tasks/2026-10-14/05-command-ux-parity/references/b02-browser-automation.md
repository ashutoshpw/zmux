# B02a browser automation bridge

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
