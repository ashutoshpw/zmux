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
