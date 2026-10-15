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

Explicit eval gaps: page CSP may block the awaited exact-script wrapper; an isolated
world fallback is not implemented and scripts are never automatically replayed. The
undefined compatibility marker retains the observed wire spelling, independently
implemented. DOMRect and Promise results are normalized without copying upstream code.
