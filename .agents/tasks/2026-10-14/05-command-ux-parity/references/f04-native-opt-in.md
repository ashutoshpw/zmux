# F04 explicit native engine opt-in

2026-10-15. Continue the accepted optional Session lane without changing the
default vt100 backend or promoting fixture limits to product budgets.

## Bounded implementation

- [x] Carry an explicit, feature-gated engine selection and caller-supplied
  mapping caps/reply lifetime through desktop launch workers, including ordinary,
  SSH and local-multiplexer PTYs. Missing configuration keeps vt100; malformed or
  unavailable selection fails visibly before any candidate child launch.
- [x] Queue owned semantic key/paste events to the sole Session worker. Encode
  against authoritative selected-engine state after earlier causal stages commit;
  keep raw byte injection distinct. Use pinned engine encoding APIs for candidate
  input, bounded output and the existing allocation/failure fence.
- [x] Route native special keys and paste through this owner path. Keep Command
  shortcuts, history overrides and IME composition in AppKit; commit composed text
  once. Retain explicit refusal of unsupported candidate application-wheel events.
- [x] Expose an explicit build/runtime opt-in through the native bridge and build
  tooling. No implicit feature-unification or default engine switch is allowed.
- [ ] Verify real Session and desktop child-observed mode-sensitive input/order,
  configuration/default/error behavior, and native source/build/smoke boundaries.
  Record red/green regression evidence and independently review the frozen slice.

## Acceptance and limits

The first checkpoint must prove default behavior remains vt100 and an explicit
candidate launch uses the worker-owned engine before its child starts. Input tests
must inspect literal bytes consumed by a real child after changing terminal modes,
including queued events behind a staged causal reply; frame booleans are not the
input authority. Errors cannot replay bytes or fall back to vt100.

The native view remains the owner of composition, selection, focus and rendering.
This slice does not close full keyboard-protocol release/repeat/IME fidelity,
application mouse tracking, richer styles, selection/VoiceOver, exact named Mac
workloads or numerical performance budgets merely by wiring an opt-in backend.
The actual implemented subset and remaining refusals must be stated explicitly.

## Ownership and coordination

Session worker, adapter/FFI input seam, desktop launch routing, native bridge and
terminal input wiring belong to the Session integration worker. Browser work owns
BrowserKeyboard/BrowserView/BrowserAutomation and the current save-panel fixture.
Coordinate App.swift before editing it; preserve all concurrent source and task
changes. Parent owns staging, commits, pushes and hosted workflow dispatch.

## Implemented source contract

`DesktopOptions.terminal_engine` defaults to vt100 independently of linked Cargo
features. Ghostty configuration requires explicit engine/general/page mapping caps
and reply lifetime; unavailable features or invalid configuration fail without
fallback. The same launch helper handles ordinary, SSH and local tmux PTYs. The
native bridge adds a strict configured constructor, while existing constructors
keep their defaults. Native startup opts in only with explicit
`ZMUX_TERMINAL_ENGINE` JSON. `scripts/build-macos-dev --with-ghostty
--ghostty-artifact-root <verified-root>` separately opts into linkage and validates
that exact retained root before replacing generated build output. It never builds
or downloads an engine and records default vt100 independently of optional linkage.

Special keys are owned semantic events with press/repeat/release and bounded
modifiers. AppKit keeps Command shortcuts, explicit history navigation and IME
composition. Its original ASCII Control-text interpretation for Enter/Tab/
Backspace/Escape is a bounded compatibility hint used only by the default worker;
the candidate uses its actual key protocol. Committed composed text and explicit
raw injection remain separate from semantic keys and clipboard paste.

The sole Session worker admits and encodes a semantic event only after any prior
staged causal generation commits. The candidate synchronizes its pinned key
encoder from the actual terminal, then applies explicit Option-as-Alt policy.
Encoder/event allocations use the existing general mapping allocator, followed by
the existing sticky quota/failure fence; no default engine allocator is used.
Paste reads the actual bracketed mode and uses the pinned bounded paste encoder,
without protocol callbacks or physical writes across FFI. Only the worker writes
encoded bytes; no failure authorizes replay or fallback.

A whole clipboard paste contains at most16384 UTF-8 bytes, with at most twelve
framing bytes. It remains one event and one bracketed unit. The existing16-item
Session queue therefore holds at most262144 paste payload bytes per session plus
one active encoded event. The32-command desktop queue can hold524288 paste text
bytes; the256-item native queue can hold4194304 paste text bytes and checks each
paste before admission. These are payload bounds, not total process memory or
accepted product budgets. Raw input retains its4096-byte chunk bound.

Only a strict Paste JSON envelope may exceed the ordinary16384-byte native limit,
up to99328 encoded bytes, allowing six JSON bytes per text byte plus fixed fields.
Parsed text stays capped at16384; Request, Command and event schemas reject unknown
fields. Other commands retain the ordinary envelope cap. BUSY retries occur only
before queue admission; accepted/partial/unknown physical paste is never retried.

Default paste preserves the existing host policy: bracketed bytes stay intact;
unbracketed CRLF collapses to one CR and LF becomes CR. The explicit candidate
uses its pinned xterm policy: unsafe controls become spaces, bracketed newlines
remain, and each unbracketed LF becomes CR (CRLF becomes CRCR). This deliberate
optional difference is not inferred from rendered mode booleans.

## Test authoring boundary and portable evidence

Public Session child-observed bytes are the primary input owner. The existing
backpressure fixture now reserves three event slots only in its release case,
while still requiring positive actual PTY backpressure and preserving deadlines.
It queues an arrow and paste behind a staged reply with old published modes; the
real child must receive current-user suffix, literal DSR reply, queued bytes,
application arrow, one sanitized bracketed paste and trailing raw POST in order.
Cancellation/deadline cases keep their original16-event setup and assertions.
These tests catch credible premature/stale-mode encoding that previous raw-byte
Session coverage could not reach. They need no test-only production seam.

A separate real default Session child verifies live application/normal cursor
modes, modifiers/repeat, ignored release, original Control hints, CRLF policy and
one maximum-size16384-byte paste. Desktop's real child independently proves
configuration reaches launch workers and feature unification does not select the
candidate. The foreign-call test owns JSON escaping, strict fields and pointer/
envelope limits, which Session tests cannot exercise. It exposed a real serde
unit-variant extra-field hole in the new vt100 config: the same focused command is
retained red before replacing the unit variant with a strict empty struct, then
green. ClipboardImageSmoke now owns original-text handoff and absence of raw paste
bypass; transformation assertions moved to the actual worker boundary.

Portable logs and negative controls are retained under
`target/agent-validation/session-native-opt-in*` and
`target/agent-validation/session-native-input-negative-controls/`. No local Linux
result establishes native source typing, AppKit execution or Darwin acceptance.
Default `scripts/check` passes293 Rust behavioral tests, strict workspace Clippy,
notice inventory and the actual C ABI executable. The linked Session/Desktop/
native-ABI package suite passes140 tests; strict default and linked scoped Clippy
pass. The existing29 linkage policy cases pass with one native-only skip.
In isolated source copies, omitting authoritative key-option synchronization
fails on real child CSI instead of SS3; forcing unbracketed paste fails on actual
unframed CR bytes instead of one bracketed unit. Restoring exact current C source
makes the same focused Session test pass. Neither negative control mutates the
working source or original engine binary.

## Optional native gate

The existing retained Darwin linkage workflow adds `include_native_host=false`;
it requires explicit Session validation and reuses the existing immutable binary
acquisition, hashes, original receipts and acceptance owner. The optional source
inventory includes all native/desktop/CLI build inputs and checked source copies.
Its357-entry input gate and existing29 policy tests (one native-only skip) pass
locally. No mutable/latest download path or engine rebuild is added.

After both original Apple slices and ARM Session cases pass, the optional helper
receives that verified root and exact source SHA, builds only an ARM native app,
retains its archive before execution, and runs an isolated transient app context.
Actual child evidence must prove initial literal DSR reply, native special-key
press/repeat routing, normal release producing no legacy bytes, one whole16KiB
clipboard frame with pinned sanitization, and raw composed-text END once. The
receipt explicitly excludes physical keyboard delivery and full IME fidelity.
The app's shared shutdown must succeed before the helper accepts its receipt.

Native typing, linking, event-handler smoke and default native regression checks
remain pending the committed, independently reviewed source dispatch. Normal
`macos-development` remains unchanged by this slice. Focus-loss release policy,
full text/Kitty key protocol/IME behavior, application mouse, full styling and
selection/VoiceOver, named Mac workloads and numeric product/performance budgets
remain open after this bounded native gate as well.

## Committed execution update

Implementation `7f125f2` is published; parent isolated validation passes all293
default Rust tests, strict Clippy, notices and the C ABI. Independent Rust/C and
Swift reviews accepted the frozen implementation. Optional native run38822344346
at `924cd89` passes four ARM adapter tests but fails the Session deadline cleanup
case with SIGKILL EPERM, observed group ESRCH and no immediate child reap. The
native host has not executed. The retained-child lifecycle repair is tracked
separately; no native input or full-parity acceptance follows from this run.

Parent startup review also found that `AppStartup.prepare` forwarded the existing
`--terminal-engine-smoke` flag to the bundled CLI, before AppDelegate could enter
the native fixture. The one-line repair admits this flag alongside the existing
native fixtures. Ordinary CLI, explicit `--cli`, empty arguments and Launch Services
`-psn_` handling retain their previous paths. The existing helper invokes the real
packaged executable and requires child observations and shared shutdown, so it owns
the pending native startup proof; no additional source-mirroring test was added.
Startup source SHA256: `12bd1ebc3a55e511f13668d76c13fbb5818dae1eddd611241bf5a7f02862464e`.

## Executed native subset at9594b98

Optional run38823543450 executes four ARM adapter and four ARM Session cases,
then builds and runs the actual ARM app. The retained child receives exactly
16,405 bytes: two application arrows, one16KiB bracketed clipboard frame and
one composed-text END suffix. Its independent digest is
`0e0bf7a3af9721fdaaf7cb24d2b60db5f4aff4ca11c43a01f27dbb495e871a5c`.
The native process exits0 after shared shutdown. Intel remains compile/link only.

Parent verified all357 retained source entries against commit9594b98, the app
archive digest, matching app/child reports and the expected complete input bytes.
Independent receipt SHA256:
`de47b1280966af49e34ae101cda343be6d9e60181d3ac62e8fcd5a79e9ba6b05`.

The whole workflow fails afterward: the completed helper report omitted the
strict reader's required schema_version. The one-line producer repair adds
schema_version1 without loosening the reader. Existing29 policy tests pass
(one native-only skip), and both helper/child Python sources parse. The committed
whole-lane rerun remains required. Physical input/full IME, mouse, styling,
VoiceOver, performance budgets and default-engine migration remain open.

## Whole optional lane accepted at525f6b9

Run38824380757 succeeds after the schema repair. Independent review verifies
all357 current-source entries against exact commit525f6b9, both retained engine
libraries and original provenance, Apple object/executable metadata, four ARM
adapter cases and four ARM Session cases. The actual ARM native app child5775
again receives exactly16,405 expected bytes with one clipboard frame; native
startup and shared shutdown exit0. No engine library was rebuilt and the default
engine remains vt100. Intel execution is not claimed.

The same run observes the cleanup transition again: child2396 is reaped on
attempt2 after7.067792ms, reporting SIGHUP exit status1. Whole-lane independent
review SHA256: `8c509f7940c3cfe8f47827c3b53cba7f89245ea066fdd796d2cb720c8dd263b1`.
Outer receipt SHA256: `a38e0b87833f98dfe0fa0684a14ca85df9c26c48fed9658f58f95e81f7ea9905`.
Default native regression, full terminal fidelity, physical input/IME, named
performance budgets and V01 remain open; no full-parity artifact was shared.
