# Status

- **State:** in-progress
- **Updated:** 2026-10-16

## Active implementation

- Screenshot-driven compact sidebar/group UI in progress. Shared group domain,
  persistence and CLI integrated (0e77cac/b4aab55/e178d25/ffc635e); full scripts/check
  passes, including strict Clippy/notices/both ABI and four group core tests.
  Native UI now uses one32pt titlebar strip, visible expanded/collapsed toggles,
  bell/workspace toggle, named collapsible headers and membership/context menus.
  Dedicated manual sidebar native suite and universal artifact runner e7c4750.
  Native compile, actual mouse/menu/sheet journey and screenshots pending on Actions;
  no native acceptance or full-parity claim yet. See N01 compact groups reference.

- Hosted Linux Foundation112e55a exposed an execution-admission fixture race:
  shell redirection creates the file before writing. Regression75ddce1 reproduces
  empty bytes with the actual admitted helper. The repair waits for authenticated
  terminal receipt and child_reaped before asserting exact output; pre-Run absence,
  supervisor lifetime and exit7 remain required. Focused red/green logs:
  `/tmp/zmux-admission-created-before-write-{red,green}.log`.
  Full `scripts/check` passes (tests, Clippy, notices and both C ABI checks);
  `/tmp/zmux-admission-publication-check.log`.

- Main3f81147 native pointer/selection run38862148076 passes35 cases, strict
  decoder and shared shutdown. Parent independently verified271 source hashes,
  archive3737c9f969a64618af8f51bb6e7382c6720bcdb5a068db4f54dd4f0e00d94ba9,
  embedded SHA/bridge and actual child8314/parent8195 raw mouse captures.
  Priorab79307 phase-c prerequisite failure remains recorded; no claim of its
  unrecorded cause. Physical mouse/optional-engine/saturation remain unverified.

- User explicitly deferred Accessibility Save-dialog acceptance and requested a
  separate disabled workflow. Regular native checks retain actual Cancel/cleanup
  and report Save-dependent cases skipped, never passed. See B01 downloads record.
- B01 production profile admission/clear/delete/reconciliation integrated from
  reviewed11f51ac/8933710. Parent verified115 retained source files, universal
  archive5ac5bdcd...e011de and actual3process/9lease native evidence38860801836.
  Combined scripts/check passes404 tests/74groups/2explicit subprocess fixtures,
  Clippy/notices/both ABI. Exact-main native revalidation remains pending.

- Native pointer changes integrated asb04b6c6/affad11/98acf98/e0fd3a8 after
  independent verification of all268 retained Git source hashes, archive integrity,
  embedded source SHA and35 native cases from38858961638 at52b5ef0.
  Shared shutdown and actual PTY1002/1003 captures pass; physical mouse,
  optional engine and sustained saturation remain unverified. Combined-main
  scripts/check passes395 tests/73groups/1existing ignored plus Clippy/notices
  and both ABI checks (/tmp/zmux-main-native-pointer-check-20261016.log).
  Hosted native validation on the integrated source remains pending.
- Mainc162b37 Foundation38858457903 passes on Linux and macOS, confirming the
  hook socket-path repair. Universal native38858458015 builds and reaches the
  known Save-panel Accessibility/post-event permission gate, then joins all
  physical owners. No TCC changes or user artifact delivery occurred.

- Hook runtime long-TMPDIR regression56a2125 reproduces the Darwin startup
  failure at the actual Desktop/PTY boundary. Bounded canonical socket selection
  and pre-bind owned cleanup pass the same five scenarios and full Linux
  scripts/check395 tests/73groups/1existing ignored, Clippy/notices/both ABI.
  /tmp/zmux-hook-long-runtime-path-check-green.log. Main43508c1 Darwin compilation
  passes but both hosted lanes stop at hook startup; the repaired SHA must rerun.

- Main94e8330 Foundation Linux passes; Darwin Foundation38856977385 and native
  build38856977404 fail on signed Darwin st_dev comparison in hook socket cleanup.
  Scoped identity normalization retains exact directory/socket replacement guards;
  Linux scripts/check395/73groups/1existing ignored and strict checks pass after
  repair. Hosted Darwin verification remains required.

- Combined mainf231186 checks pass395 tests/73groups/1existing ignored plus
  strict Clippy, notices and both native ABI checks. Log:
  /tmp/zmux-main-hooks-pointer-check-20261016.log. This validates integrated
  adapter+pointer owner source; native/provider/full-product gates remain open.

- A02 opt-in Claude adapter integrated locally as036e478 from reviewed718c896.
  Exact ten source hashes verified. Isolated checks394/72groups/1existing ignored
  plus Clippy/notices/both ABI pass. Real Desktop/PTY synthetic-provider tests
  exercise holds, no idle claims, no PTY body input, uninstall and same-request
  bounded retirement recovery. Actual-Claude acceptance remains unverified.
- T01 semantic pointer worker integrated locally asb03f371/f231186 from
  c53e150/1b6c0cd. Real raw-child unknown-command red and six mode scenarios green;
  worker-authoritative encoding shares wheel reports. Isolated checks388/72groups/
  1existing ignored pass; independent protocol/lifecycle review found no blocker.
  Native event wiring and packaged pointer regression remain active separately.
- Full main46422ef workflow38854903325 passes universal compile, then fails Save
  dialog preflight because accessibility_trusted=false/post_event_access=false.
  All physical owners joined. Raw log /tmp/zmux-main-46422ef-native-failure.log.
  This repeats the known runner permission gate; no unchanged rerun requested.
- Readiness producer38855036920 at45f4d9 passes fresh dependency/silent owner,
  public consumer, package and archive controls. Exact retained bytes are under
  review before admission or native startup activation. Native configuration
  preparation70f5aa has52 source hashes independently verified, but no AppKit
  or final-bundle acceptance yet. Cloud health3530 remains HTTP200/statusok.

- Exact-main46422ef terminal-selection run38854903069 passes32 native clipboard
  cases, decoder and shared shutdown. Independently verified266 source hashes,
  archive/source plist/linked bridge and actual app/child identity. Foundation
  run38854903306 passes. Full-product/native acceptance remains separate.
- Native inbox baseline63e6ab9 run38854940772 reaches the intended missing
  Agent Inbox entrypoint failure after real window focus, global/closed-recipient
  message records and zero terminal-input prerequisites pass. Production inbox
  implementation continues in its isolated worktree.

- T01 wrapped-word selection integrated as `a6c8e3b`/`a14d94d` after actual native
  red `38853562903` and green `38854246653`. All32 clipboard cases and shared
  shutdown pass;266 source hashes/archive/bridge verified. Production terminal
  sources are byte-identical to accepted6c58fb0. Exact-main native gate remains
  separate; no full-parity or user-facing artifact claim.

- A02 provider owner prerequisites integrated locally at `97965e3`: single-message
  context claims, exact consumer retirement, durable unconfirmed attempt records,
  explicit duplicate-risk retry and automatic replay suppression. Successful fresh
  retirement/retry fences cached local handoffs; replayed mutation receipts cannot
  fence a newer consumer. Read/failed messages with unknown attempts are not evicted.
  Combined `scripts/check` passes 387 tests, 1 existing ignored, strict Clippy,
  notices and both ABI checks. Adapter and native inbox remain separate active work.

- Main `69ff15c` Foundation run `38852633864` passes both macOS and Linux.
  Native run `38852633869` completed universal compilation, then the packaged
  journey stopped at the Save driver Accessibility/post-event permission gate.
  Shared shutdown joined all physical owners. No generic -4 appeared in this run;
  the earlier BUSY remains unresolved. No artifact/full-parity acceptance follows.
  Raw log: `/tmp/zmux-main-69ff15c-native-failure.log`. Local cloud
  health at `http://127.0.0.1:3530/health` returned `{"status":"ok"}`.
- Active isolated follow-ups: exact-window native agent inbox; Claude provider
  exposure/retirement/retry owner; connected browser profile clear/delete; atomic
  readiness-capable dependency admission/native opt-in. Constructor generation,
  cached-retirement and unknown-attempt retention review findings are being repaired
  before integration. Wrapped-word selection regression at `e54e3bb`, Actions
  `38853562903`, exercises the actual PTY/native clipboard boundary before repair.

- A02 Rust/CLI durable inbox integrated locally as `d429636` (behavioral red) and
  `8c54b39` (reviewed fix). Original owner checks passed 368 tests plus strict
  Clippy/notices/ABI. Review repairs cover retained-send replay, cached lease expiry/
  held state, and policy-writer executor stalls. Authorization linearizes at the
  fsynced handoff; in-flight foreign-superseded responses are explicitly bounded
  by the original TTL. No provider hook, wake or native inbox claim. Integrated
  main verification passes 379 tests, 1 existing ignored, strict Clippy, notices
  and both foreign ABI checks.

- Main `22550c1` Darwin Foundation checks pass in run `38851088457`, including
  the five real execution-admission tests. Linux Rust/Clippy/notices/ABI passed,
  then the DOM prerequisite failed because Chrome `--version` timed out with zero
  output. Failed Linux job rerun once unchanged passed (attempt 2); retained raw log
  `/tmp/zmux-foundation-22550c1-linux.log`. Full native build remains separate.
- Observer4 consumer passes its isolated Linux suite but stays off main until
  a matching readiness-capable dependency is admitted and bundled. Source review
  identified a finite-completion race at transport-progress validation; repair
  and a deterministic Monitor regression are underway. Existing v6 cannot satisfy
  the new capability, so integrating the consumer alone would break native launch.
- A02 review repairs cover offline discovery, immutable target resolution,
  sender attribution after moves, and retained send receipts after target close.
  Final claim exposure still needs expiry/held-state fencing and nonblocking policy
  admission before integration. B01 production clear/delete foundation is authorized
  in isolation after accepted physical lifecycle evidence and a real control red.

- Foundation run `38850609492` at `b5557cb` exposed a Darwin-only test socket
  mode error before all five execution-admission handshakes. Accepted fixture
  sockets now explicitly use bounded blocking reads, matching production owners.
  Full local `scripts/check` passes; new hosted Darwin verification remains pending.

- V2 runtime/history cohort integrated locally through `58fbd92`, preserving the
  two regression commits. All 42 touched files exactly match reviewed `b15fb47`.
  Darwin package run `38849188824` passes 51 executed owner tests with 293 source
  hashes, 9 phase logs, retained dependency and app archive verified. Review receipt
  `83221b29e52c2b3639b35e9ffad8ecae33e1b096a26e39636b820c8652d7345c`.
  This accepts executed ARM64 runtime scope; observer4/InputReady, Intel/minimum OS
  and default activation remain separate. Integrated full local checks pass 357 tests,
  strict Clippy, notices and both foreign ABI checks.
- B01 physical clear/removal run `38849912294` passes at `538ada1`, with all six
  source copies verified: mounted rejection preserves A, exact inactive A store is
  empty before UUID removal, fresh A remains empty and B cookies/storage survive,
  then both public cleanup callbacks and actual owned exits pass. Production
  clear/delete admission and recovery owner is now being planned from this evidence.

- Main `15aee7b` pushed with `e595515` window stress/backpressure repair; local
  checks pass 346 tests, 1 ignored. Native full run `38848268523` at `4afb224`
  builds universal successfully, passes earlier browser/workspace checks, then fails
  the native download Save-panel driver preflight: Accessibility trust and event
  access are both false on the hosted runner. No TCC changes or weakened assertions.
  Requested self-hosted macOS runner labels asynchronously; independent work continues.
- B01 baseline run `38849145986` at `1d3bbb2` passes cookie retention and strict
  public cleanup after the fresh-B helper lifetime repair. This is a no-removal
  fixture, not production clear/delete acceptance. Next isolated clear/removal
  scenario retains B and independently checks A clear before namespace removal.
- Input readiness Linux dependency and actual Desktop/CLI silent-first-line cases
  pass; actual Darwin dependency run `38849682804` at `d36bc91` remains pending.
  Execution-admission native package run `38849188824` at `b15fb47` is separate.
- A02 queued-message restart regression has a committed red and passing first
  implementation through the real CLI/socket/two PTYs. Consumer generation and
  handoff hardening remains in progress; no full messaging parity claim.

- Main `scripts/check` exposed intermittent terminal-capacity rejection in the
  40-generation native close-before-open-ack test. Its admission predicate waited
  for success after a valid rejection, ignoring asynchronous terminal retirement.
  Focused and whole-window reruns passed unchanged; the test now explicitly retries
  only that bounded capacity rejection with fresh request IDs, preserving all 40
  accepted lifecycle generations and rejecting other errors. Full `scripts/check`
  passes, including strict Clippy, notices and both foreign ABI checks.

- Main `4afb224` is pushed; Foundation run `38848268839` passes. Full macOS run
  `38848268523` remains in progress. The native window fixture now expects saved
  schema 7; its remaining lifecycle assertions are unchanged.
- V2 history run `38847303782` attempt 2 passes both five-stage native history
  scenarios and actual Models decoder. All 261 retained source files match Git;
  archive integrity verified. Receipt `be6af63b2bef7217c2c861edd82c0dc793cdac3465bd50f144c37e2ab2ad147d`.
  This accepts saved-history presentation only, not live Darwin runtime admission.
- Darwin V2 test repair is underway in isolation: preserve actual process lifetime
  while allowing Python launcher re-exec, bound closed-socket EOF and owned cleanup.
- A02a design reviewed and isolated Rust/CLI implementation authorized. First gate
  is actual durable queued-message restart with recipient terminal input untouched;
  provider handoff, native inbox and full body-size parity remain separate work.
- Input-readiness dependency v2 source reviewed; isolated build and actual frontend
  controls authorized. Transport connection alone must never admit user input.
- Browser profile caller-retention diagnostic `38848628624` runs at `421b797`.
  It changes caller store lifetime only; strict fresh-reader cookie checks remain.
  Production clear/delete and cookie persistence acceptance are still open.

- Corrected selection `38847301845` at `e9484ab` passes actual decoder and 24 native
  UTF-8 clipboard cases. All 255 retained source hashes and archive integrity verified;
  receipt `564de5478f093c81fe129858d8e640c774105c13408c1b0f131ae0d40aea74bf`.
- Browser disposition source accepted after never-dispatched guard repair; integrated
  as `a5f87a8` / `3c5c8c3` / `7cc53ba` / `1bb05b1`. Main Rust/CLI/Cargo sources exactly
  equal the branch that passed full checks; coordinator merge preserves accepted
  history ownership. New native execution remains required; not yet pushed.

- `a7f06dd` Foundation run `38846559334` passes on both platforms; its full native
  build is still running. `8d7e6e8` exact-main history run `38846275441` passes all
  five AppKit stages with 254 retained source hashes verified. Main `e9484ab` fixes
  the selection decoder's missing real BrowserProfileLeases source; failed selection
  `38846273113` stopped at compilation before any cases. Corrected selection
  `38847301845` is running with unchanged assertions.
- V2 runtime/metadata committed on diagnostic branch as `39bb666`, native projection
  as `89ffb94`, decoder closure correction `a2ac7a5`. Exact-source review and full
  local checks pass; actual helper five, channel guards four, public/native Rust
  seventeen and legacy observer two one all pass within recorded revision scopes.
  Native history `38847303782` and retained-package runtime `38847140793` are running.
  These commits have not been integrated into main or accepted for native behavior.
- Browser obsolete-event disposition is under independent review; a missing
  never-dispatched terminal-receipt guard is being repaired before integration.
  Frontend InputReady producer and observer4 consumer work remain isolated.

- Published `8d7e6e8` includes accepted V1 native history. Exact-main Foundation
  `38846237751` stopped at rustfmt on one diagnostic assertion; formatting repaired.
  Fresh full local `scripts/check` passes 343 tests/one ignored, strict workspace
  Clippy, licenses and ABI. macOS full/selection/history checks are running; no
  full-parity artifact handoff. The format fix does not change test behavior.

- Finite completion integrated locally as `1a81f2d`, deterministic exec regression
  `f80a432`, and repair `f2fd5b6`. Exact frozen cohort passes 22 desktop tests,
  16 actual Zellij/auth tests and strict relevant Clippy; unchanged broad
  `scripts/check` evidence is 340 passed/one ignored plus ABI/license checks.
- Native saved-history run `38845311766` at `af80731` passes actual ARM64 build,
  exact Unicode clipboard, read-only typing/Paste, scroll/selection retention
  across cross-window move and source closure, and awaited shared shutdown.
  All 253 retained source hashes match. Independent review/integration underway;
  these generated historical receipts do not establish live authentication.
- Wrapped-copy repair `3203dca` has 24 exact native clipboard cases and decoder
  coverage. Browser asynchronous mounted-store repair `ce5e057` passes its native
  mount and lease checks. Both are included in the pending main publication cohort.
- Browser diagnostic `9518593` / run `38844359603` reaches seven native smoke stages
  then fails the still-required Save-dialog Accessibility preflight. Metadata
  identifies 17 removed-target detach errors; two late failure receipts still need
  exact rejection attribution. No suppression or ownership relaxation applied.
- V2 execution admission and V8 persistence are being implemented in isolated
  `ci/execution-admission`. Core/desktop 42 focused tests and strict Clippy pass;
  actual helper Run/no-Run and finite-exit evidence, native decoding and integration
  remain required. Frontend semantic input-readiness design is recorded separately.
- Cookie persistence diagnostic `137255d` / run `38843434898` loses both native-created
  and JavaScript cookies across a fresh process even without deletion; localStorage
  persists. Profile clear/delete remain unavailable; root cause is unresolved.
- Local Docker readiness remains at `127.0.0.1:3530`; identity/devices/organizations
  are ready, VM/relay/email unavailable. Public tunnel acceptance remains open.
- Full 26-package/172-family parity is incomplete. No subset artifact handoff.

- `ac41162` passes foundation38839050321 on macOS and Linux. Native38839050463
  passes Swift typecheck, early native fixtures and Rust/C ABI; universal build
  remains running. Earlier native38838984041 was cancelled by workflow concurrency.
- Wrapped-copy diagnostic5a71574/run38839319267 built the actual app, then failed
  compiling the standalone decoder probe because actual production dependencies
  were omitted. No native clipboard cases executed. Reviewed harness correction
  links the real model dependencies and exact archived bridge; rerun pending.
- Browser public all-data-clear + UUID-removal diagnosticade61da/run38839388593
  passed same-process empty-A/intact-B checks with exact source and owned-process
  receipts. Fresh-process6e71df3/run38839907151 kept A empty and B localStorage,
  but lost B cookie; no deletion-isolation acceptance follows. A no-clear baseline
  with raw cookie/store observations is being prepared to distinguish fixture
  persistence from cross-profile mutation.
- Real finite completion regression committed locally `a170f29`; completed-history
  no-launch restore regression committed `2a16e93`. Both intended red receipts are
  retained; publication waits for the corresponding reviewed green implementation.

- R03 runtime/checkpoint prerequisite committed `dc43608` after all 21 staged
  source hashes matched the independently reviewed freeze. Isolated `scripts/check`
  passed 331 tests (one intentional ignored); actual Zellij conformance passed 12.
  Normal finite completion, semantic input readiness and AppKit activation remain
  open. Foundation38838984070 and native38838984041 started on that exact SHA.
- Native smoke continuation committed `ac41162`: independent rendering, remote
  clipboard, tmux and key checks precede the still-required Save-panel AX gate.
  Bounded stage receipts and no-prompt preflight retain the actual failure;
  missing host permission is not a pass. Source reviewed, Python syntax valid;
  Swift and native execution pending Actions.
- Browser lifecycle diagnostic `4f6a602` failed its unchanged reopened-profile
  assertion: localStorage was cleared but cookie `probe=A` remained after the
  successful public removal callback. The other profile retained its data.
  Profile deletion remains open; a public store-data clearing correction is
  being designed and must pass the original isolation assertions.

- Published `12d5c14` passes foundation38836210021 on macOS and Linux, including
  unchanged Chrome DOM coverage. Exact-main selection38836229618 passes18 native
  clipboard cases; independent review binds235 source files, archive/plist,
  focus/PTY identity and shared shutdown. Scope remains visible selection only.
  Full native38836209900 passed core/C ABI/universal build and window fresh-process
  reopen (including todos and split ratio). Browser profile rename, zoom, fill,
  click, interactions, edit/scroll, keyboard Shift and topology checks logged pass.
  It stopped at the save-panel Accessibility gate (runner untrusted). No full smoke
  receipt exists. A second diagnostic relaunch then returned BUSY and timed out;
  this does not prove the original shutdown cause. Runner repair removes diagnostic
  relaunch and adds bounded original-process/shutdown observations. Retained ZIP
  SHA2562439b767f080ab5ee122155019c0e60a287b7c4d7885507e890facebcec988e8.
  An asynchronous question asks for any existing AX-enabled self-hosted Mac labels;
  independent work continues without assuming that hardware is available.
- Browser native lease acquisition committed locally `78aa188`: fixed OS Library
  lock namespace, asynchronous exact-token store construction, process-lifetime
  ownership and cancellation-before-construction release. Full323 isolated checks
  pass; parent caught and reviewed the repaired macOS temporary-root assumption,
  followed by focused original/new C ABI green. Swift/WebKit execution remains
  pending; clear/delete and physical lifecycle parity remain open.

- Cohort `38698c3`: native38835681008 passed native typecheck/early fixtures,
  then failed the buffer fixture because a published pane can precede its first
  screen. The fixture now polls only the exact startup error before READY, retains
  strict failures afterwards and uses a bounded deadline per observation. Isolated
  real CLI/PTY tests (2) and strict Clippy pass; native rerun remains required.
  Foundation38835680975 passed macOS, but Ubuntu Chrome `--version` timed out
  before DOM tests started. Prior green used the same runner image; no DOM
  regression or underlying startup cause is established. Check remains unchanged.
  Selection `b659b73` and the updated deferred-cloud plan await the next push.
  Cloud `/ready` rechecked successfully at port3530.
- Published cohort `3fc2364` passes foundation38832687268 and native universal
  compilation, then native38832687280 stops in WindowLifecycleSmoke's metadata
  predicate. Earlier move coverage retains status `move`; later preparation adds
  `temporary` but incorrectly expects one status. Fixture now checks both exact
  key/value entries and emits bounded predicate diagnostics on timeout. Native
  validation remains pending: local Swift check requires unavailable xcrun. Broad
  browser keyboard/profile tests and fresh-process reopen were
  not reached; no native acceptance is inferred from compilation or saved todos.
- Native Zellij observer38833738922 at `4b96b48` passes 35 actual owner tests.
  Independent source/package/phase verification is complete; the acknowledgment
  revision remains synthetic, so native S01 durability and AppKit Start/Attach
  remain open. Correction committed on main `c1b4d38`; consumer implementation now
  coordinates actual Session ownership with exact durable checkpoint completion.
- Named buffers committed `ca2d815`: exact private UTF-8 persistence, concurrent
  writers and shared raw/bracketed paste. Isolated314-test baseline plus focused
  final parser checks, strict Clippy, notices and C ABI pass. Historical output,
  grapheme counts, capacity compatibility and native execution remain open.
- Browser lifetime-lease primitive committed `21741d3` after source review and
  isolated314-test/full checks. No native store acquisition or clear/delete is
  exposed. Integration must use the common WebKit namespace, independent of
  configurable catalog paths, and retain physical leases across logical shutdown.
- Selection baseline38833749334 at `6b52949` reproduces the actual Command-C
  failure after verified native focus. Owner repair38834961185 at `3b686c1` passes
  all18 native cases; independent review verifies225 source copies, focus/PTY
  identity, exact UTF-8 clipboard results and shared cleanup. Frozen integration
  committed `b659b73`; seven full-file hashes match and only selection startup hunks
  were staged from shared App.swift. Current-main native acceptance remains open;
  no full-parity acceptance follows from this slice.
- Attach-only v6 recovery identity core committed `7b1943b`. Combined323 tests in
  67 groups, formatting, strict Clippy, notices and C ABI pass on the isolated
  reviewed source. S01 durable checkpoint coordination and actual observer Session
  consumption remain in progress; no UI Start/Attach is exposed yet.
- Local readiness still returns200 at3530 (identity/devices/organizations ready).
  Public zmux.w3dev.app DNS remains unresolved here; VM/relay/email unavailable.
  Full V01 and the user-facing artifact handoff remain open.
- Reviewed browser literal-key repair committed as `08eaee8`: implicit Shift now
  remains consistent through paired key events, held keys and alias releases.
  Four frozen source hashes and parent review verified; native green pending.
  Publishing the cohesive main cohort for exact-head native validation; no
  full-parity artifact handoff is authorized by these partial checks.
- Selection baseline38831911526 again stopped before selection: app inactive,
  visible but not key after bounded activation. This is fixture admission failure,
  not a selection regression. Public AppKit word-boundary probes were retained;
  launch ownership is being reassessed before another run. Observer package
  run38832049927 remains separate from the main validation cohort.
- Current main `a85dc43`: default macOS38827930379 passes Swift and Rust/CABI
  gates and the universal build, then fails packaged browser keyboard acknowledgment:
  trusted events did not match the prepared sequence. Browser diagnosis is active;
  no runtime browser acceptance is claimed.
  Idle38827961978 completed idle-one, then failed during idle-eight warmup because
  macOS ps column padding changed the rendered start timestamp. The observer
  whitespace correction is in progress; no resource budget acceptance claimed.
- Zellij native-package38827122318 passed bundle build/signature/version and
  owner-0/owner-1, then failed owner-2 on Darwin PENDIN kernel state after restore.
  A reviewed test-only candidate preserves production TCSANOW and compares all
  configurable settings, with a private-PTY queued-input exactly-once probe.
  Diagnostic `9ec70f7` native38828839028 passes: downloaded package and source
  receipts verified, 30 owner tests and the 22-byte Darwin PENDIN probe pass.
  Test correction committed locally as `ff13144`; GUI Start/Attach remains open.
  Sampler correction `aac6677` idle38829001880 passes with verified 221 source
  files, 201 measured samples, exact child identities and shared shutdown.
  Accepted correction is local main `ee3351f`; one virtual-host run is not a
  physical-Mac resource budget. Main native checks were preserved.
- Local main commits through `6949d8f` (push held for keyboard repair): checklist
  persistence `498f01b`, profile rename `551e501`, reference-compatible final-tab
  move `7fb59d9`, and authenticated Zellij observer `6949d8f`. Persistence/profile
  combined310 checks pass; final-move69 scoped cases and observer311/full plus10
  feature cases pass. Frozen source reviews and exact index hashes recorded.
  Native v5 save/reopen, profile rename and observer execution remain pending.
- Final-tab reference review removes the temporary refusal: retiring an emptied
  workspace retires its checklist, leaves destination data untouched and creates
  no move recovery entry. Exact close-history capture remains; real2PTY movement,
  disk and fresh-reopen tests pass. No invented checklist archive is required.
- Browser diagnostic `28b72ce` run38829898297 establishes case6 press! inserts!
  but trusted keyup becomes1 with Shiftfalse. Host/document/focus stable; first5
  acknowledged actions pass. Mapping repair now owns implicitShift correctly;
  exact native green remains required. Current DOM148 rerun was unavailable due
  headless startup failure; no pass claimed. Bounded diagnostics preserve assertions.
- TERM009 baseline `501cdca` run38830756527 compiled but stopped at keyWindowfalse
  before selection (not intended red). Focus-only `1f3d492` run38831911526 uses
  canonical activation and actual focus wait; baseline TerminalView unchanged.
  Expanded selection implementation is separate and unaccepted pending real cases.

- Native38827114277 at7b passes Swift/focused native checks, then fails in
  shutdown fixture directory creation (AlreadyExists). No browser acknowledgment
  execution occurred. A process-local atomic suffix fixes parallel same-clock
  fixture naming; fixed-clock old naming reproduces the failure, repaired naming
  and exact restored source each pass all three existing shutdown tests.
- Todo slice `fc47e98` adds scoped checklist commands and native activity
  projection; 300-test/CABI validation and parent review pass. Persistence and
  actual native acceptance remain open. Idle harness `1a28205` is independently
  reviewed after rejecting partial sample coverage and changed process starts;
  actual Darwin one/eight-terminal measurements remain pending.

- Published through `7b6540e`: browser trusted-event completion `de4becd`,
  opt-in retained ARM Zellij packaging `2b5d40f`, and admitted-child PR fixture
  repair `7b6540e`. Each has scoped checks and independent source review;
  browser and repair each pass isolated297-test/CABI checks. Package admission
  and capture each pass nine checks, plus full core checks and actual retained
  ZIP/negative controls. Native38827114277 and package38827122318 are running
  on that exact commit. Preserve useful runs; no full-parity artifact handoff.
- The PR fixture negative control catches direct-child-only cleanup with an
  actual still-running descendant; the finite control exits naturally, without
  signalling an adopted PID. Exact production code and full fixture then pass.
  Runtime review receipt: `/tmp/zmux-a01c-admission-independent-review-20261016.json`.

- Native38825900209 at76618a0 passes Swift6/focused native checks and eight
  metadata cases, then fails the lookup deadline fixture because its gh child
  marker is absent. The250ms shared deadline includes preceding Git/tool cleanup;
  the fixture did not prove admission before asserting descendant cleanup.
  Diagnosis and a deterministic boundary repair are in progress; production
  timeout behavior has not been shown faulty. Browser acknowledgment source is
  independently reviewed; isolated required checks are running before publication.
- Cloud38825955954 at1507939 passes. The local Docker service was rebuilt with
  backup-before-migration and returns health/ready200 at127.0.0.1:3530; the
  post-start path-to-port check returns3530. Public zmux.w3dev.app still fails DNS
  resolution from this host; no public authentication acceptance is claimed.

- Published PR metadata `76618a0`: repository-verified URL/number handoff,
  exact workspace/window ownership, bounded child cleanup and native row/inspector
  projection. Final 297-test check and independent source reviews pass. Required
  native PR receipt is enforced; macOS38825900209 is running. Full PR flags,
  automatic refresh and broader A01 remain open.
- Published HTTPS cookie repair `1507939`: real auth/PG regression reproduced
  non-Secure cookies for HTTPS development origins; repaired table and all18
  service integration tests, typecheck, Docker build and source review pass.
- Default native38823748479 at52736fc confirms native browser key delivery but
  fails because its snapshot overtakes dispatch completion. Bounded trusted-event
  acknowledgment is implementing; no keyboard or overall browser closure claimed.

- Optional engine38824380757 at `525f6b9` is green and independently accepted:
  357 exact source entries, ARM adapter/Session execution, real native semantic
  input and shared shutdown. The Darwin fallback is observed a second time
  (attempt2, 7.067792ms). Default native/full fidelity/performance remain open.
- Both-host Zellij packaging38823056442 at986600f passes; downloaded source,
  notices, binaries, rebuilt modules and all negative/owner checks are verified.
  Opt-in native bundling is now implementing; no default or GUI integration claim.

- Optional native38823543450 at9594b98 executes the repaired pending-to-reaped
  Darwin transition and actual ARM app semantic input/shared shutdown successfully.
  Parent verifies357 exact source entries and complete child-input digest. The
  overall workflow fails afterward on the helper report's missing schema version;
  producer-only schema repair retains the strict consumer and awaits full rerun.

- Published `52736fc` includes runtime/startup repair `9594b98` and failure-only
  browser keyboard diagnostics. Default native38822345401 at924cd89 passed the
  browser-focus round trip and reached keyboard input, then failed its snapshot
  ordering assertion. Default native38823748479 and optional engine38823543450
  are running. Shutdown diagnostics38823545800 pass parallel/serial cases with
  actual ECHILD; they did not observe the rare EPERM/ESRCH fallback.

- R03 `986600f` separates structured Cargo stdout from download diagnostics.
  Actual cold-Cargo red/green, seven stream/lifecycle checks, fresh producer, owner,
  13 negative-package cases and independent review pass. Exact isolated
  `scripts/check` passes; both-host packaging38823056442 is running.

- Published `924cd89`: explicit optional terminal engine and worker-owned semantic
  key/paste input (`7f125f2`) pass parent isolated `scripts/check` (293 tests, strict
  Clippy, notices and C ABI). Optional Darwin38822344346 stops at process cleanup
  before native host execution; retained-child reap repair is in progress. Default
  native38822345401 separately validates the document-focus fixture repair.
  Neither result establishes full terminal fidelity or V01 acceptance.

- H01 authenticator/recovery API and web UI pass 17 real PostgreSQL tests and
  15 real Chrome checks, including pre-fix sign-out red/repaired green. Independent
  review, TypeScript, production Docker image and parent core checks pass.
  Hosted service checks38821504830 pass at0eb2198. Local Docker promotion and
  12-table backup restore rehearsal pass; public/native callback remains open.
  See [MFA](references/h01-mfa.md).

- H01 native credential fix `352ba3b` passes the early actual macOS HTTP/Keychain
  red/green gate in run38818645717. The same fixture rejects the pinned baseline
  at cancelled-login persistence and accepts the repaired client with external
  cleanup. Independent retained-log/source review accepts that focused gate; the
  full run stopped at the separately tracked sidebar smoke predicate.
- R03 package implementation `65471de` passed independent exact source/notice
  review and isolated `scripts/check`. Opt-in both-host run38819411938 is building
  rebuilt plugins and source/notice packages; no default acquisition changed.
- F04 shutdown diagnostics38818223456 at `74a2287` passed actual Darwin ECHILD
  assertions and three parallel/three serial repetitions, independently reviewed.
  Earlier intermittent graceful-close timeouts are not declared repaired.

- A01b adds bounded app-wide `current` inspection from one published Desktop
  snapshot: exact identities/revisions, hidden terminal local-child lifecycle,
  metadata/attention counts and explicit unknown activity. Real CLI/control
  red/green, all 19 focused/existing CLI cases, catalog probes and strict Clippy
  pass. Cached lifecycle is not an active task or remote liveness. Native packaged
  execution, Cloud/task/resource knowledge and full A01/TOPO-027 remain open.
  See [current inspection](references/a01-current-inspection.md).

- Optional Session38814746696 at `c9ec29b` is accepted for its four public ARM
  Session cases plus four adapter cases. Independent artifact/source review verifies
  both Apple executables, same-shim linkage and unchanged retained engine libraries;
  Intel is compile/link/inspection only. Observed partial-input, exact reply ordering
  and cleanup outcomes pass. The rare EPERM/ESRCH reap fallback was not observed;
  all-group extinction, native-app/default/typed-input, minimum-OS execution and
  performance acceptance are not established. See [F04](references/f04-session-engine.md).
- Main `15e8600` includes the reviewed Save panel SDK compiler repair and early
  both-target driver typecheck. Main38818219763 later reached packaged sidebar
  smoke and failed its compound keyboard/AX/persistence predicate. Repair1d8c3f9
  uses the bound window preference key and separates diagnostics; run38820239578
  is pending acceptance. Actual Save panel UI and browser keyboard remain pending.
  Workspace metadata and other independent parity slices can proceed while those
  runtime gates and separate Zellij packaging/notices work continue.

- Native38813108591 at `f62a10c` passes the focused physical-storage regression,
  both strict Swift target checks, Rust/core/ABI and universal build. Its packaged
  close/save/reopen journey passes: exact identities, fresh process/native input,
  restored split ratio and all physical owners joined in both phases. Artifact
  `12009264859` and its ZIP hash were independently verified; see [S01](references/s01-session-persistence.md).
  The overall run fails later while compiling the separate save-panel fixture;
  broad smoke, browser keyboard, active remote/download cleanup and full V01 remain
  unaccepted. Native38812458734's exact raw-name EILSEQ refusal remains explicitly
  recorded; production lossless decoding and strict Rust path policies are unchanged.
- Patched Zellij38811822523 Darwin producer and all27 actual owner checks pass with
  independently verified source/workflow/log receipts. GUI integration, distributable
  dependency packaging and acquisition pins remain separate gates.

- Published `a2e3d09`: native physical-storage repair `0ffc110`, Session backpressure
  fixture `a865c62`, bounded diagnostics and shorter Zellij prefix-shadow fixture.
  Parent combined `scripts/check` passes286 tests, strict Clippy, notices and C ABI.
  Default terminal engine and Zellij acquisition pins remain unchanged.
- Native38809101107 at `11671a4` passed universal build, then reopening failed with
  ENOTDIR in the strict state-directory walk; saved session/generation bytes stayed
  unchanged and close owners joined. Foundation can strip `/private` from an existing
  path. NativeBridge now passes a POSIX realpath physical spelling; source review passed.
  Native38811810709 passed both Swift typechecks and the focused main filesystem cases,
  then failed to create an invalid-UTF8 fixture directory. Native38812458734 captured
  errno92/EILSEQ; the corrected focused fixture and packaged reopen now pass in38813108591.
  Browser keyboard remains unverified because its later smoke stage was not reached.
- Optional Session38811818507 proved Darwin accepts1022bytes of the first4096byte
  input chunk before EAGAIN. The old fixture incorrectly required one complete chunk.
  Repaired fixture retains exact child byte ordering and positive partial-write proof;
  local linked4 pass and independent review passes. Native rerun38812468146 pending.
- Patched Zellij producer passed both hosts in38809114500; Linux actual27 owner checks
  passed. Darwin passed suspend/resume/input/detach, then the test's104byte prefix-shadow
  socket exceeded its OS limit. The98byte fixture keeps the same real rejection check;
  both-host rerun38811822523 pending. No native GUI integration/distribution claim.

- Native38808070074 at `c8bf104` passes both strict Swift typechecks but stops in
  the download physical-credit fixture before app execution. The fixture now advances
  admission time to expire logical tickets without draining worker completions, then
  observes actual credit release for at most5s instead of assuming a100ms worker sleep.
  Removing the physical-credit guard fails the strengthened pre-drain assertion.
  Exact isolated `scripts/check` passes286 tests, strict Clippy, notices and C ABI;
  independent review passes. Browser keyboard/shutdown/reopen runtime gates remain open.
- Foundation38808070193 at `c8bf104` passed both Linux and macOS jobs.

- `52440da` adds explicit optional Session engine ownership, staged publication after
  physical PTY reply writes, and cancellation guards before publication/user writes.
  Exact combined browser/fixture/Session source passes `scripts/check` (286 tests,
  strict Clippy, notices, C ABI); linked Session4 and independent review also pass.
  Default engine remains unchanged; Darwin Session runtime acceptance is next.

- Browser keyboard delivery is committed at `7765206`: real events target the owned
  WebKit surface, with held-key lifecycle and deadline/ownership checks immediately
  before delivery. Parent combined `scripts/check` passes286 Rust tests, formatting,
  strict Clippy, notices and C ABI. Production native keyboard execution remains pending.
  `068b56c` makes two asynchronous test fixtures observe actual receipt/completion;
  original operation deadlines and no-replay checks remain intact.
- Retained-engine linkage run38806117440 at `be52324` passed: both Apple targets
  compile/link, ARM executes4 adapter tests, Intel is compile-only. This does not
  establish Session/native default-engine or terminal fidelity acceptance.
- Local readiness rechecked: identity/devices/organizations ready on127.0.0.1:3530;
  VM/relay/email unavailable. Public tunnel setup remains the user's separate step.

- `313da45` adds the typed Swift fixture repair, bounded process failure receipts,
  and both-architecture early Swift typechecking. Exact combined isolated
  validation passes286 Rust tests, formatting, strict Clippy, notices and C ABI.
  This batch includes `7a81cc4` retained-engine linkage and `15ecbdc` shared
  shutdown; hosted verification is the next gate. Browser production keyboard
  and worker-level terminal engine integration remain separate work in progress.

- `f3fc91e` native38804157702 passes Rust/ABI and resolves the selector collision,
  then Swift rejects an overly complex `SurfaceOrderSmoke.selection` expression.
  An explicit typed-loop repair is in progress. Foundation macOS38804157739
  reaches a descendant-process fixture failure; Zellij38804187126 reaches actual
  terminal input then exits. Both need bounded failure diagnostics before a
  runtime cause can be claimed. No new reopen acceptance evidence exists.

- Local commits `7a81cc4` and `15ecbdc` add the opt-in retained terminal bridge /
  Darwin linkage lane and shared shutdown with native cleanup barriers. Parent
  combined frozen-tree `scripts/check` passes286 Rust tests, strict Clippy,
  notices and C ABI; independent reviews pass. Push held while the useful
  `f3fc91e` native run reaches close/save/reopen. Native compile/lifecycle and
  new Darwin shim execution remain pending; default engine is unchanged.

- `f3fc91e` publishes independently reviewed Darwin accepted-socket mode repairs
  for production Zellij bootstrap and the CLI browser fixture, plus unambiguous
  AppKit tab selectors. Combined isolated `scripts/check` passes; targeted actual
  fragmented-frame regression has red/green evidence. Hosted native38804157702,
  foundation38804157739 and Zellij38804187126 are running; acceptance pending.

- Latest published `0101763`: foundation Linux passes; macOS exposes inherited
  nonblocking mode in a CLI browser socket fixture. Native Rust/ABI passes, but
  Swift compilation stops on ambiguous AppKit `moveLeft` selector before reopen.
  Narrow repairs are in progress; there is no new restore acceptance evidence.
- Browser keyboard probe `38802980621` passes all 16 native cases on ARM macOS15,
  including detached WebKit, trusted defaults and intercepted Command-W resend
  without changing the unrelated editor. Production integration and cross-request
  modifier lifecycle remain pending.
- Shared shutdown is under independent review: one 10s deadline, retained physical
  owners, native download barrier and responsive pending-cleanup presentation.
  Review caught SSH barrier ordering and download finalization reservation races;
  repairs and owner-boundary verification precede native acceptance.
- Opt-in terminal bridge frozen source passes isolated fullcheck283 and four
  linked engine tests. Hosted new-shim validation will reuse immutable libraries;
  default engine migration and complete terminal fidelity remain open.

- `ac28cf2` repairs two Darwin socket-readiness fixture assumptions and twelve
  immediate notification snapshot reads. Hosted `38800293490`/`38800305050`
  expose the former, foundation `38800293474` exposes `Busy` in the latter.
  Actions, denial/loss/hash assertions and notification expectations are unchanged;
  only bounded read/pump readiness is retried. Combined isolated fullcheck280 and
  independent source reviews pass. Native revalidation remains pending.
- Keyboard `38800309699` compiles but traps during exact-self activation before
  dispatch. `6807cec` adds opt-in bounded LLDB diagnostics against unchanged Swift;
  the actor-boundary hypothesis is unconfirmed without a stack. No browser key
  delivery or hidden-host conclusion follows from either failed setup run.

- `824bb9f` repairs production Zellij Unix socket path limits using checked
  canonical `/tmp` and unchanged private namespace ownership. Kernel/public CLI
  long-TMPDIR regressions fail before the fix and pass afterward; isolated
  fullcheck280 passes. `f14ceee` adds exact-self activation to the keyboard probe;
  repaired native execution remains pending. These slices do not close V01.

- Published `629bb2b` includes the validated Darwin child/process repair,
  exact upstream-library macOS13 / consumer macOS14 inspection policy with
  hash-pinned retained ARM64 reuse, and an isolated public WebKit keyboard probe.
  Each source freeze passed fullcheck280; native outcomes remain separate.
  Current runs: native38799167366, foundation38799167465,
  Zellij38799176839, engine38799180282, keyboard38799183833.
  Native/foundation fail on a Darwin Unix socket path exceeding SUN_LEN;
  the owner lane independently reproduces the production temporary-root issue.
  The keyboard probe compiles in strict Swift6 and executes all16 cases, but
  lacks a key-window precondition, so no keyboard behavior is accepted.
  Engine verification fails in its native linker fixture before artifact work;
  the standard-runtime linkage repair is published as `41f1645` and retry
  `38799601657` passes all hosted steps, including retained ARM reuse,
  both Apple consumer links and ARM execution; independent artifact inventory
  review remains pending. Intel execution and application linkage remain open.
  No full-parity artifact is accepted or shared.
- Local control-plane `/ready` freshly returns HTTP200 with identity/devices/
  organizations ready. VM, relay and email remain unavailable; public tunnel
  and authentication callbacks remain unverified.

- Darwin process repairs filter nonterminal observations from exit-only child
  polling and use the correct safe peer-credential API. Workload admission now
  waits boundedly for the selected canonical shell before registry binding,
  retaining PID/start/parent/boot checks and no-replay semantics. Fullcheck280,
  realCLI5 and parent child2/broker3 pass; hosted repair acceptance remains pending.

- Hosted batch `adf5132`: native `38797286030` fails Rust compilation on the
  Darwin peer-credential API path before reaching reopen. Zellij owner
  `38797306939` passes Linux but Darwin exposes stopped-child observations in
  exit-only polls. Repairs and a newly reproduced pre-exec workload admission
  race are under validation; no native acceptance claimed.
- Artifact `38797303343` retains the ARM64 library but rejects its declared
  minOS13 against requested14. Pinned upstream overrides the requested version
  on Darwin. Revised exact effective-library13/consumer14 policy and retained
  artifact reuse are under review; no application engine switch.

- Browser type/select/scroll now have bounded admitted actions and DOM behavior
  checks, including value versus text-content beforeinput ordering. Parent
  combined fullcheck passes280 Rust tests, strict checks and C ABI; independent
  DOM140 and source review pass. Native WebKit execution remains pending.

- R03 foreground local-zellij start/attach/list/status now uses kernel-verified
  ownership, a retained workload supervisor and shared child lifetime management.
  Independent Linux24 checks include real detach/reattach, input/resize and signal
  handling. FIFO blocking and SIGINT cancellation have red/green repairs. Parent
  combined fullcheck passes280 tests plus standard checks. Native production
  workflow is pending; ordinary native tabs and broader lifecycle parity remain.

- Reopen diagnostics now retain bounded public status/persistence/profile reads
  and the generated fixture session/allocator JSON, including failure artifacts.
  Isolated fullcheck271 and focused capture/receipt probes pass. This adds evidence
  for the next native run; it does not claim to repair the unresolved restore fault.

- Same-pane tab reordering now uses shared final-position planning, explicit
  focus (defaultfalse), CLI anchors/legacy syntax, native menu/shortcut/context
  actions and guarded same-pane drag. Isolated fullcheck passes271 Rust tests
  and standard checks; parent core/real-socket/focus/persistence checks pass.
  Swift/native execution and physical AppKit drag remain pending.
- Native `38794349738` at `9a30dec` passes universal build and final-close save
  with completed asynchronous shutdown. Reopen then stalls with an empty model;
  startup/restore diagnosis is active. Packaged acceptance remains failed.

- Darwin terminal artifact recipe independently passes17 integrity/real Clang
  Mach-O checks, source recipe verification and isolated full `scripts/check`
  (`/tmp/zmux-darwin-parent-check.log`). Manual workflow retains two thin macOS14
  slices, source/patches/notices and receipts; actual hosted build, inventory review
  and ARM64 runtime acceptance are pending. No production engine switch.

- Browser double-click, hover, DOM focus, check/uncheck and centered-scroll aliases
  now share admitted document-bound actions. Parent integrated `scripts/check`
  and all106 Chromium DOM cases pass. Browser startup-focus precondition repaired
  after diagnostics reproduced a missing focus event before document readiness;
  exact event assertions remain. Original opaque failure remains unattributable.
  Packaged WebKit execution is required and not yet claimed.

- Foundation `38794349737` at `9a30dec` passes Linux but exposes a macOS
  notification fixture snapshot `Busy` at line880. The suite now retries only
  snapshot publication contention through its existing bounded helper, and fails
  other snapshot errors immediately. Notification assertions and commands remain
  unchanged. Independent review accepts; integrated `scripts/check` passes
  (`/tmp/zmux-browser-notifications-integrated-check.log`). Hosted green pending.

- Native `38791991724` at `10b8f1b` passes universal compilation and reaches
  final-window termination. Its stack exposes a nested AppKit termination loop
  retaining the main actor before asynchronous shutdown can run. Termination is
  now scheduled through a run-loop selector; isolated `scripts/check` passes
  (`/tmp/zmux-termination-runloop-check.log`). Native confirmation is pending.
- Numbered workspace shortcuts/browser focus (`bb2b98c`) and reviewed static
  terminal artifact provenance/notices (`2360fb0`) pass isolated checks.
  Full parity remains incomplete; these are incremental implementation slices.

- `01b28e7` Foundation passes both hosts; universal native build passes, packaged
  startup again stops at initial makeKeyAndOrderFront. The bounded sample shows
  an idle AppKit run loop, not a blocking Rust/PTY stack. Retaining one menu tree
  per native window repairs repeated submenu reparenting on key callbacks; causal
  confirmation requires the next native run. Parent isolated full check passes.
- Browser click committed `8b66276`: 261 portable tests, strict checks, 57 registry
  probes and independent 62-case DOM check pass; native execution remains pending.
- PTY `38791084172` at `1abc98d` passes Linux and Darwin, including actual partial
  writes, cleanup and the two intentional failure controls. Production terminal
  engine selection is unchanged. Numbered workspace selection and remembered
  browser focus are under implementation/review; R03 foreground ownership is active.

- Workspace ordering committed `b4fcf6e`: parent full isolated `scripts/check`
  passes after repairing a prompt-sensitive cwd fixture with exact framed actual
  pwd output. Independent ordering and fixture amendment reviews pass. Native
  execution remains pending. Browser fill is committed separately as `0dc50f0`.
- Native `38787910086` universal build passes, but packaged startup stalls after
  bridge startup and before initial window activation completes. Per-call traces
  and bounded sampling of the exact isolated test executable are added; no timeout
  extension or second app launch. Zellij `38787914932` passes on Darwin at
  `11258fe`; PTY `38787910315` passes Linux but still fails Darwin, under diagnosis.

- Browser fill committed `0dc50f0`: exact 13 frozen source hashes, full isolated
  check (256 Rust tests, Clippy, notices and C ABI), independent review and parent
  Chromium 48-case DOM run pass. Google Chrome 151 failed to start the fixture;
  Chromium 155 completed it. Native WebKit execution remains pending.
- Workspace ordering is frozen against `0dc50f0`; independent review and parent
  isolated integration check are running. Native `38787910086` at `11258fe` passed
  Rust/C ABI and is building; pushes are held to preserve its lifecycle evidence.
  PTY `38787910315` and Zellij `38787914932` remain separate hosted gates.

- `c01c3bc` Foundation passes Linux and macOS (`38786746911`). Native
  `38786746932` failed strict Swift compilation at the zoom fixture's CGFloat
  string conversion; explicit Double conversion is repaired and isolated
  `scripts/check` passes again. The earlier window-lifecycle timeout remains
  unresolved; its new traces have not yet executed.
- PTY `38786746778` identifies Darwin cleanup deadline with master still open;
  master-before-wait repair passes independent focused Linux acceptance. Zellij
  `38787432280` confirms authentication repair but exposes short-read drain
  throttling in its TERM-output fixture; bounded waiter repair is in progress.

- Integrated workspace moves, corrected browser zoom and bounded native lifecycle
  diagnostics pass fresh `scripts/check`: 255 tests plus strict Clippy and C ABI.
  CLI moves default to no focus; native geometry follows workspace UUIDs.
  Zoom accepts finite factors and clamps to0.25–5. Same-window move semantics,
  configured-default zoom reset and exact-source native execution remain open.
- Hosted `16cf61a`: Foundation passes Linux/macOS; native universal build passes
  but packaged window close times out150s with empty logs. New phase diagnostics
  are prepared. Isolated PTY passes Linux, fails Darwin cleanup after reply/flood
  success; direct-child cleanup repair passes independent focused Linux checks.
  Zellij's real client-exit repair is being checked independently; an unrelated
  credential fixture rejection/send race is now reproduced and being repaired.

- Checkpoint `16cf61a` pushed: native lifecycle startup routing, process-group
  ownership cleanup, isolated kernel PTY acceptance and authenticated Zellij
  rejection fixture. Native `38784161310`, Foundation `38784161346`, and PTY
  `38784165438` are running. Zellij `38784160950` passed the repaired negative
  fixture but failed real attachment cancellation: terminal child did not exit
  successfully. Diagnosis continues; its Darwin gate remains open.
- Independent final Linux PTY acceptance passes frozen8/lock31, construction
  exhaustion cleanup and named negative controls. This is an isolated experiment,
  not a production terminal-engine switch. Workspace moves remain in progress.

- T03 two-window creation/focus/close, per-window rendering/attention and last-close
  save/reopen slice passes independent full portable checks and native source review.
  Packaged macOS execution remains required; moves/groups/canvas and full parity
  remain incomplete. Native3438565 failed an existing deadline-fixture ENOTCONN
  assertion; repaired without changing timeout or no-replay behavior.

- Local Docker service refreshed at `58a882b`: private backup, persistent migration002
  and expiry index verified, bounded cleanup codes0/tokens0, health/readiness200.
  Post-health `bunx path-to-port` confirms3530; tunnel/public auth and VM/relay/email
  remain open. See [local cloud](references/local-cloud.md).

- Latest checkpoint: Foundation `38778435301` passes at `c33ec40`; macOS
  `38778435343` passed universal compilation but failed save-panel destination
  selection in packaged smoke; fixture investigation continues.
  Full parity remains incomplete; no final parity artifact exists.
- F04 metadata hosted `38777353391` passes Linux/macOS; all29 inputs verified
  against `b86a700`, receipts recorded in `90f05e3`. Production engine unchanged.
- Zellij retained authenticated Quit passes independent real Linux rerun
  `/tmp/zmux-zellij-retained-parent-20261015/receipt.json`. Darwin short-lived
  kill-session credentials remain unsupported; repair committed/pushed `58a882b`,
  hosted `38779766351` confirms daemon exit and client exit0 but fails on59
  undeliverable opaque bytes during shutdown. Typed terminal-outcome handling is
  being implemented; no lossless shutdown claim.
- T03 two-window core/runtime/native and CLI are under validation, including
  close-before-open acknowledgement, physical capacity retention and key-window attention.
  Isolated real-PTY replies experiment continues separately.

- F04 isolated metadata owner committed/pushed `b86a700`; independent full39+7
  and final focused normal/source/arena controls pass, all29 final hashes checked.
  Hosted Linux/macOS `38777353391` running; no production engine switch.
- Native coordinator extraction `bded4e2` committed; downloads file-owner fixes
  passed hosted filesystem probe in `38776244720` at `eaf8cda`; app compilation
  and packaged WebKit acceptance still running. Earlier runs failed file-path
  handling then a Swift fixture isolation boundary; both retained strict guards.
- Zellij Darwin public SDK/peer and descendant containment now execute, but latest
  `38777058699` fails PTY failed-exec evidence and exact-close timeout. Diagnostics
  show exit111 with no retained PTY bytes, live server after helper exit0, and
  ENOTCONN relay closes. Owner is repairing per-direction lifetime and stage evidence.
- F01 contract schema frozen for independent review/parent checks. Two-window
  vertical lifecycle implementation and isolated real-PTY terminal design are active.

- Browser downloads committed `0bda3f1`: independent portable full checks/review pass;
  packaged Darwin file/WebKit acceptance remains pending. Zellij public-peer fixture
  committed `4d09a03`: independent real Linux transport/cleanup rerun passes;
  manual-only Darwin workflow awaits dispatch. Neither closes its full package.
- Persistence repair `610e897` passes Foundation `38773695057` and 128 fresh
  fixtures in diagnostic `38773735612`. Pre-fix also passed 128 attempts, so root
  cause remains unproven. Native `38773694848` passed universal compilation and packaged smoke; downloaded
  archive integrity/source/flags verified. Downloads run `38775130649` and Zellij
  public-peer run `38775157527` are active at pushed `d629406`.
- Next active slices: shared native coordinator extraction, opt-in bounded terminal
  metadata capture, and executable CLI compatibility contracts. Local Docker
  `/health` and `/ready` on port 3530 remain healthy; VM/relay/email remain unavailable.

- T03 singleton ownership committed8705dea and integrated terminal experiment
  committed93daf02; both pushed. Parent isolated full checks pass. Foundation
  38771928593 passes. macOS38771925429 fails before Swift compilation in the existing
  concurrent persistence lock test (ENOENT opening generation lock); focused owner
  investigation is active. Do not weaken lock outcomes or call this native acceptance.
- Integrated hosted experiment38771957372 passed Linux/macOS arm64; parent verified
  all23 input hashes against93daf02. Independent Linux rerun also accepted30 normal
  stages and five intentional assertion failures. Production engine remains unchanged.

- Native d6d9cc8 run38769334202 and Foundation38769333991 passed. Downloaded
  diagnostic archive/source/checksum and three images verified; profile manager,
  terminal keys/scrolling and durable tmux reopen now have hosted native receipts.
  Full V01 remains incomplete; no subset artifact is presented as final delivery.
- F04 resource/effects run38769399834 passed Linux/macOS; all17 input hashes
  verified against d6d9cc8. Single-owner composition is still under implementation.
- B01 downloads, T03 singleton window schema/durable migration, F04 integrated
  terminal ownership and R03 exact Zellij attachment design are active.

- B01 native profile manager committedbf1a4b3 after parent full scripts/check,
  foreign ABI checks and independent review. Create/list/open use the shared catalog;
  native manager execution and rename/clear/delete remain open.
- F04 isolated resource experiment committedf468782 and passed hosted38766300795
  on Linux x86_64 and macOS15.7.9 arm64; all14 downloaded input hashes match the
  commit. Mac raw universal compilation passed, but resource execution is arm64 only.
  Exact history/allocation-denial checks do not prove whole-app RSS or engine parity.
- Native528be92 andf4f99d3 both failed Swift nested async fixture cleanup compilation.
  The extracted-helper candidate1868dcc passed its standalone Mac diagnostic;
  full native38767602739 remains pending. Foundationf4f99d3 passed38765601911.
- R02b app-bound tmux attachment, reuse, persistence/reopen and typed caller routing
  pass full parent scripts/check and independent real-tmux/schema review. Model5 and
  sessionv3 retain old session readers. Native acceptance and remote tmux remain open.
- Browser profile presentation/menu and bounded browser runner committed/pushed528be92.
  Parent isolated scripts/check passed; nine subprocess lifecycle cases and35 Chromium
  DOM cases passed. Native38764217483 is pending; this repairs the missing-datastore
  Swift constructor exposed by38762433983, but native success is not yet established.
- Full V01 and its GitHub Actions macOS artifact remain the objective. Intermediate
  archives are diagnostic/acceptance evidence, not full-parity delivery.
- B01 explicit profiles committed94725f1 after207 portable tests, strict Clippy,
  notices/C ABI and independent review. Native profile isolation/reopen and relaunch
  remain open. R02b app attachment/restore is implementing against the new schema.
- Native948e922 passed universal build/packaged smoke in38759082621; archive checksum
  and exact source metadata verified. It predates B01 and file-backed key setup7035ee8.
  The intermittent multiline setup corruption remains open despite this pass.
- New cloud expired-credential cleanup passes12 PostgreSQL integration tests and fresh
  review in a disposable database. Persistent service database was not migrated/cleaned.
- Native5f5005e passes universal build and packaged smoke in38753451286: replies,
  sidebar pointer/keyboard resizing, metadata inspector, browser live title, special
  terminal keys and managed file-URL/refusal cases all executed successfully.
- Downloaded5f5005e ZIP checksum/integrity/source verified; AppKit and WebKit images
  inspected. Full-window compositor capture remains unavailable on hosted runner.
- B02b document locators/readiness committed/pushed6af571f. Parent full193 Rust tests,
  strict Clippy/notices/ABI plus35 real Chromium DOM cases pass. Foundation38754685035
  passed. Native38754684779 passed browser journeys but failed the later key fixture.
  Diagnostic38757351964 captured mangled Python input/SyntaxError with the intended
  terminal focused; input-path investigation is in progress, not a browser focus repair.
- F04 isolated engine feasibility passed Linux x86_64 and Mac arm64 execution plus
  both Mac architecture compile/lipo checks in38756429963. No production engine
  replacement or resource-budget claim. Native page allocation/history policy remains
  unresolved; an isolated compatibility exporter is in progress.
- R02 headless local tmux committed57c9df4; app-bound attachment/reopen/restore is in
  progress. B01 persistent browser profile binding is being validated independently.
- CI fixture isolation repair2435687 uses a process-local sequence as well as time/PID.
  Native38758505493 exposed a separate startup snapshot Busy race in the session test;
  the test now retries only Busy within its original deadline. Isolated scripts/check
  passes, including the real CLI session lifecycle, strict Clippy and C ABI.
- Local Docker cloud healthy at3530, confirmed with path-to-port after health checks.
  Public tunnel unverified; VM/relay/email unavailable. Hosted deployment remains deferred.

## Delivered slices

- R01d committed/pushed (`c260185`, compiler repair `ee8952f`): raw image paste/drop
  classification, bounded off-main PNG preparation with orientation, exact-session
  executor receipts and worker-finish source cleanup. Fresh reviews passed. Parent
  checks pass (79 Rust tests plus C ABI); universal compile and packaged native
  ImageIO/cache/pasteboard/view-to-shell smoke pass at `ee8952f` (`38720024342`).
  Diagnostic archive SHA/checksum and native render were inspected. Physical/remote
  image coordinator, directory/custom-transfer, stress and full R01/V01 remain open.
  See [receipt](references/r01-clipboard-images.md).


- R01c committed/pushed (`d7db8fe`), final fresh review passed: private authenticated terminal channels; bounded regular
  file uploads, progress/cancel and origin-bound insertion; native file drop/Finder paste.
  Local checks passed with 78 Rust tests plus C ABI. Hosted macOS exposed a Darwin poll
  incompatibility (`-1` descriptors); repair `c9cd45c` now passes Linux/macOS Foundation
  checks (`38716147517`) and universal/native packaged smoke (`38716147327`). Raw clipboard images, directories,
  two-host/physical/native acceptance and full R01/V01 remain open.
  See [receipt](references/r01-file-transfers.md).

- R01b committed/pushed (`ec9422e`), fresh review passed: public `zmux ssh`, validated wire routing, real SSH PTYs,
  no-focus creation results, endpoint-preserving tabs/splits/reopen without startup replay.
  Full local checks pass (73 Rust tests plus C ABI), including isolated real OpenSSH.
  Connection sharing, upload/drop/paste and full R01/V01 remain open.
  See [receipt](references/r01-ssh-workspaces.md).

- R01a descriptor foundation integrated: validated SSH endpoints/jump routes,
  host-key/config defaults, forwarding, bounded options and argv. Eight descriptor tests
  pass; full workspace checks pass (65 Rust tests plus C ABI). This established descriptors only; the R01b entry above records subsequent launch integration. Fresh review passed. See [receipt](references/r01-ssh-descriptor.md).

- F03a committed/pushed (`a1c621f`): local socket access/password modes, credential precedence
  and private-file validation, executor admission revocation and deny-by-default remote
  policy. Full checks pass (57 Rust tests plus C ABI); native/managed/resource gates
  remain open. Fresh repair review passed. See [receipt](references/f03-socket-authorization.md).

- F01a committed/pushed (`c572484`): shared command registry, grouped/task/leaf offline help,
  guide/skill alias, executable JSON catalog and app capabilities. Parser regressions
  for literal stdin markers and malformed browser help repaired; full checks pass
  (49 Rust tests plus C ABI). See [receipt](references/f01-command-registry.md).
  F01/full parity and native gates remain open.

- F02a committed/pushed (`d1393cb`): live UUIDs/typed refs/indices, scoped target resolution on
  the workspace executor, caller defaults, revision guards and real shell context.
  `scripts/check` passes (47 tests plus C ABI); fresh repair review passed. Native gates remain; F02 and
  whole parity are incomplete. See [receipt](references/f02-target-routing.md).

- Audit-driven closure plan prepared: 26 dependency-ordered packages cover all 172
  audited families, with commit slices, verification/artifact gates and first SSH/transfer
  delivery batch. See [execution plan](references/gap-closure-plan.md). Package execution
  remains planned; this update changes no runtime behavior.

- Broader whole-product source-family audit completed at cmux `3ba653b9` / zmux
  `69342a8`: CLI/global contracts, terminal/topology/native UX, browser/content/settings,
  agents/attention/automation/remote, cloud/platform/security/distribution. All 428 initial
  command rows and 171 shortcut rows have review routing; source-only namespaces and RPC
  declarations are separately cataloged. See [audit](references/full-parity-audit.md).
  This completes the requested broad audit, not implementation or per-option runtime proof.

- Corrective slice: terminal wheel/history navigation, typed-input return to live screen,
  worker-owned mouse protocol routing and exited-process history; focus/activation attention
  acknowledgement and Dock unread count. Local regression and workspace checks pass;
  macOS Actions compile and packaged smoke pass; artifact downloaded and verified.
  CLI repair backlog recorded in scroll-attention-repair.md.

- P1: independent terminal tabs; ⌘T/⌘W/⇧⌘T and Recently Closed; move/reorder, grouped closes,
  fresh-shell restoration and retryable launch failures. Hidden sessions remain alive.
- P2: collapsible sidebar with saved visibility, Workspaces/Notifications tabs, attention badges,
  bounded bell/OSC/explicit notification state and generation-safe read/open/dismiss/clear.
- P3a: embedded public WebKit surfaces without PTYs, address/navigation/find/zoom controls,
  retained hidden-tab state, CLI open/navigate, frame-owned split layout and ratio retention.
- Original MIT implementation; no imported or translated cmux GPL/BUSL code/assets.

## Verification and download

Current native runtime `ee8952f37dfe3dbe145e7423a66a7a1558da6dd4` passes universal
arm64/x86_64 compilation and packaged native rendering/image smoke (`38720024342`),
including 79 Rust tests plus C ABI in that job. Foundation Linux/macOS passed at
`c260185` (`38719421495`); the intervening repair changes only Swift failure literals
and task notes. Downloaded diagnostic ZIP passed SHA-256/integrity and source matching;
its native render was inspected. These are subset receipts; V01 remains false.

R01c runtime `c9cd45ca41ac256356329f7b7c2b668d3938ce48` passes all 78 Rust tests,
fmt/Clippy/notices/C ABI on Linux and macOS (`38716147517`), plus universal native
compilation and packaged rendering smoke (`38716147327`). The workflow archive is
diagnostic only; V01 is false and no new user-facing artifact is shared.

Current foundation evidence: `c572484` (run `38706071891`) and `a1c621f`
(run `38707343689`) passed macOS universal compile and packaged smoke. R01 descriptor `d06818a` passed Foundation checks (`38707919043`) and macOS
compile/packaged smoke (`38707919132`). R01b `ec9422e` passed Linux/macOS Foundation checks (`38711025578`) and
macOS universal/native smoke (`38711037026`).
These are diagnostic receipts; V01 remains false and no new artifact is shared.

The next artifact shared with the user must pass full feature parity (V01). Existing
links below are historical evidence for earlier subsets, not the requested new delivery.

Accepted runtime source: `70cd3b614e0b3f9cf0fcaa0e33bec4c59c228d7a`.
[Download macOS build](https://github.com/ashutoshpw/zmux/actions/runs/38692821187/artifacts/11952308942).
Local checks and hosted universal/native smoke pass; archive and both captured images inspected.
See [delivery receipt](references/native-delivery.md) for checksum, evidence boundaries and repairs.

## Open scope

- P0 executable per-leaf/option contracts, shared command registry, UUID/typed refs,
  caller defaults/auth implementation and measured Mac resource budgets remain open.
  The broad source comparison is complete; discovery and source review are not completion counts.
- Whole P1/P2/P3 phases remain incomplete: multiwindow/focus history/groups, original pane restore
  anchors, richer attention/hook/status operations, browser automation/profiles/downloads and
  remaining UX/command contracts are tracked in their references and phase plan.
- P4–P8 persistence/customization, automation/content tools, agents, remote/cloud and platform
  behavior remain open. Reopening creates fresh local shells or remote logins; job/session resurrection remains open.
- Interactive drag/IME/VoiceOver, Intel runtime, actual HTTP/TLS/permission journeys and numerical
  resource/remote-stress evidence remain open. Cloud callback smoke is not live cloud sign-in.
- User revised cloud hosting on 2026-10-15: hosted workflow deployment is postponed;
  run locally via Docker/PM2 and derive the service port with `bunx path-to-port`.
  Local Docker service is healthy at http://127.0.0.1:3530 with PostgreSQL over validated
  TLS on loopback port 3531. Parent checked /health, /ready and /v1/config. The user will
  map https://zmux.w3dev.app through Cloudflare Tunnel; public routing/auth remains unverified.
  VM, relay and email capabilities remain unavailable. Local setup changes await review.

## Next action

Validate the remote image paste coordinator and physical provider behavior, then
continue native file-URL/symlink/refusal and custom-upload contracts plus real two-host
transfer acceptance. Directory refusal and regular-file symlink semantics are already
covered by the existing R01 backend; recursive directory push belongs to H03. Continue remaining F02/F01/F03 contracts and the
F04 terminal-engine/Mac-budget evaluation alongside it. Preserve all working subsets.

User correction on 2026-10-15: incremental builds must not substitute for full source-based
parity. Prioritize the managed SSH + remote file/image transfer journey and complete owner/
effect/error tracing for every family. SSH is not blocked by cloud hosting. See the
[SSH/transfer comparison](references/ssh-transfer-contract.md). No runtime SSH support
was added in this comparison. The broader audit is now recorded in the reports above.

Implement the audited P0 command registry/target contracts and the managed SSH/transfer
priority journey, alongside remaining P1 navigation/window/recovery actions;
then close P2 attention and P3 browser gaps against the linked ledgers. Preserve native artifact
and visual gates, and commit cohesive validated slices. Full parity is still the objective.

## 2026-10-15 local hosting and native verification

- Local Docker launcher reviewed and environment override defect repaired; service healthy
  at http://127.0.0.1:3530. Hosted deployment deferred by user; public tunnel unverified.
- macOS Actions run 38725306919 at 85e6e9f passed universal build and packaged native
  smoke after shifted-bracket shortcut correction. No full-parity artifact handed off.
- Session persistence integration and crash-safe allocator work remain in progress.

## S01 foundation accepted locally

- Versioned private topology persistence, transactional empty-desktop restore, CLI receipts
  and native startup integration implemented. Durable allocator blocks prevent crash reuse.
- Review repaired stale OSC7 cwd persistence and silent final-save failure; independent
  runtime reproductions pass. Parent scripts/check: 123 tests plus Clippy/notices/C ABI.
- Native hosted checks pending; S01 additive/multiwindow/full-browser and safe cancel gates
  remain open. Next implementation begins B02a browser automation bridge.

## B02a shared browser bridge

- Browser navigation/read/eval/snapshot CLI now uses bounded native work receipts and
  host/document/restore fencing; shared menu/toolbar navigation enters the same owner.
- Parent 136 Rust tests, strict Clippy, notices and C ABI pass. Fresh reviews accepted
  shutdown cancellation, viewport and lost-event recovery repairs. Public native smoke
  authored; hosted Swift/WebKit execution pending. B01/B02 and V01 remain incomplete.
- Persistence Foundation/macOS checks at5f26fbe passed; no partial artifact handed off.

## Browser followup and N01a in progress

- `8ee0ee3` compiled universally on macOS; packaged browser smoke failed and Foundation
  exposed a temporary-directory collision. Neither run establishes full acceptance.
- No-focus browser creation and fixed-source CSP preflight are implemented; parent
  portable checks passed. Native followup roots its pending Promise, checks no replay
  across content worlds, and requires input progress before evaluation finishes.
- Local cloud health/readiness reverified HTTP 200; path-derived tunnel target remains
  `http://127.0.0.1:3530`. VM, relay and email remain unavailable.
- N01a native desktop notification ownership, permissions and exact-token actions are
  being implemented. In-app inbox behavior already exists; OS banners are not yet proven.

## N01a native attention integration

- Browser followup at `50309f9` passed Foundation and real macOS universal/packaged
  checks, including CSP/no-focus/pending-eval journeys. B02 advanced scope remains open.
- Native attention now captures epoch/namespace/session, routes confirmed exact actions,
  reconciles OS desires, and keeps denied permission separate from inbox access.
- Reviews repaired dropped restrictive permission observations and unbounded inbox
  action tasks. Native permission smoke runs its isolated runtime before the main app
  context, avoiding two owners of the same socket. OS banners remain a separate gate.
- Broader N01 CLI metadata, scoped operations and reply are not yet closed.

## N01b and transport verification

- N01b committed locally at `2619b56`: subtitle/defaults/desktop preference and scoped
  notification operations. Parent full check passes 161 tests plus Clippy/notices/ABI;
  fresh review accepted. Native UI and permission smoke await a green hosted run.
- `3ab1355` macOS runs exposed Darwin rejecting socket option updates after peer close
  while reply bytes remain buffered. A nonblocking, readiness-polled transport repair
  passes parent full Linux checks; hosted validation remains pending. No macOS notification acceptance is claimed yet.
- N01c typed workspace notification destinations are next. Full V01 remains open.

## Darwin repair and N01c

- `619a3eb` completes nonblocking/poll dependencies and peer-close regressions. Parent
  full Linux check and fresh review pass; Foundation38735209799 passes Linux/macOS.
  Native38735209748 passed Rust/ABI and is compiling; notification smoke still pending.
- N01c typed workspace destinations, CLI provenance and native inbox routing are in
  progress, with owner and public CLI regressions observed red before repair.
- Local Docker health/readiness rechecked HTTP200 at http://127.0.0.1:3530.

## N01c local verification and native crash diagnostics

- Typed workspace destinations and shared native routing pass 169 Rust tests, strict
  Clippy, notices and C ABI. Fresh production review accepted; external-caller smoke
  environment isolation repair is included before publication.
- `619a3eb` passed universal native compilation but packaged execution exited SIGTRAP
  before evidence. Empty stdout/stderr and absent immediate crash reports do not identify
  a cause. The workflow now captures a bounded LLDB crash backtrace; this diagnostic
  cannot satisfy the native smoke gate. Source-only actor-isolation guesses are unproven.
- N01d UUID handles and shared newest-openable-unread navigation are designed next.
  Replies, agent last-prompt events, richer unread state and full V01 remain open.

## N01d in progress

- `2dcb038` commits workspace notification ownership and failure-only native debugger
  diagnostics. Foundation38736550252 passes; macOS38736550307 is compiling.
- UUID handles preserve numeric compatibility; shared jump-to-unread will serve CLI,
  menu, inbox and Command-Shift-U with activation only after confirmed selection.
  Runtime/native integration and verification are in progress.

## N01d verification and confirmed native repair

- N01d full local check passes 172 tests plus strict Clippy/notices/C ABI. Public CLI
  handle/navigation cases pass; fresh production review accepted.
- `2dcb038` universal compilation passed but packaged smoke failed. LLDB proves the
  UN call-out queue hits a Swift main-actor assertion in refreshPermission's callback.
  Explicit Sendable completion boundaries repair the identified cause; next hosted
  execution must verify it. No partial artifact is shared.

## N01a-d hosted integration accepted

- `c9db69c` passes Foundation38739600484 and universal/packaged macOS38739600592,
  including permission ordering, metadata, workspace destination and unread navigation.
  The captured callback assertion is repaired on the actual native path.
- Downloaded archive checksum/integrity/source verified; AppKit and WebKit images
  inspected. Full-window compositor capture remains unavailable on the hosted runner.
- Local cloud HTTP health/readiness still pass at3530. Public zmux.w3dev.app did not
  resolve from this host on2026-10-15; tunnel setup remains external.
- N01e bounded PTY write receipts are in progress before reply integration. V01 stays
  open and no subset artifact has been handed off.

## N01e phase1 locally accepted

- Bounded PTY input receipts pass 175-test parent full verification and fresh race/
  cleanup review. Queued expiration cannot write later; active partial/unknown results
  never authorize replay. Complete establishes local PTY acceptance only.
- Next: immutable terminal reply opt-in and bounded core receipt polling, then native
  text-input category and confirmed outcomes. Native/remote reply parity stays open.

## Current integration at13504c3

- Full local checks pass after A01 integration and deadline-fixture correction.
- df3f8ff Foundation38749608190 passed; macOS38749608349 failed during a CLI test
  before native compile. The test incorrectly required body-phase expiry, although
  header-phase expiry under load also preserves the same absolute deadline contract.
- At13504c3 the test accepts either response phase, explicitly requires deadline expiry,
  and retains original timing limits, ordinary success, empty-output and no-replay checks.
- macOS38750257118 must establish actual metadata/title/sidebar/key execution; it is
  not yet an acceptance receipt. Browser readiness/locators and native R01 file-URL
  journeys are in progress. Engine evaluation is gathering source-backed alternatives.
- Local cloud health and readiness rechecked HTTP200; path-to-port again returned3530
  after health. Public Cloudflare Tunnel and VM/relay/email remain unverified/unavailable.

## Native diagnostic at700648d

- macOS38751027370 passes universal compile and reaches packaged smoke. The sidebar
  trace proves width/preference264 and correct hit routing, but AppKit promotes the
  divider to first responder on mouse-down. An event-specific focus repair is in progress;
  keyboard/AX focus and strict assertions remain required.
- First Darwin sampler artifact downloaded and inspected: two startup RSS samples,
  one successful status probe and one owner-unverified skip. This verifies instrumentation,
  not a performance budget; see F04 resource reference.
- R01 native file-URL/refusal acceptance committed004c05a after fresh review and full
  scripts/check in an isolated index snapshot. Native execution pending; not pushed yet.

## 5f5005e native acceptance

Run38753451286 produced a valid universal archive with SHA256
637944de020a31c9f337a28f2fb3125a6c928069512a305f8e4e17c1710081a6.
The parent verified archive integrity, source metadata, all required smoke flags and
both captured images.15 resource/CLI samples cover18.50 seconds of this mixed smoke;
these are instrumentation receipts, not controlled workload performance budgets.
Full-window screencapture status1 remains explicit. No subset artifact was handed off.

## Local tmux without a UTF-8 locale

- Bug: with LANG/LC_ALL/LC_CTYPE unset, `local-tmux list/status/start` failed with
  "tmux returned invalid session metadata". Reproduced on Linux tmux 3.2a: tmux rewrites
  the tab separators (and also 0x1f/0x01, so a control-character delimiter would not help)
  to `_`, e.g. `SESSION:$0_1792146830_dev`. The public CLI regression
  (`headless_cli_reads_session_state_without_a_utf8_locale`) removes the three locale
  variables, also pins `LC_ALL=C`/`POSIX`, and failed for exactly this reason before the fix.
- Fix at the owner (`local_multiplexer.rs`): every captured tmux invocation passes `-u`
  (attach specs for the user's terminal are unchanged). Query parsing is a pure
  `parse_query`; invalid session/client metadata errors now carry a locale hint and a
  control-escaped copy of the raw line bounded to about 200 characters (unit tests).
- `start` rolls back when anything after creation fails before registration: it kills
  only `=name`, guarded by the incarnation marker, and kills a server only if this
  operation started it and no session remains. The original error is returned; a
  rollback failure is appended. The registry-save-failure path is unchanged.
  Failure injection wraps the real tmux (tab rewrite, refused kill-session) in
  `crates/remote-runtime/tests/local_multiplexer.rs`; three tests cover new server,
  existing server with foreign/managed sessions intact, and failed rollback. Disabling
  the rollback or `-u` makes the corresponding tests fail.
- Rollback ignores the cancellation flag (still bounded by `OPERATION_TIMEOUT`, registry
  lock held), so a start cancelled after `new-session` still removes its session/server;
  a cancelled or timed-out `new-session` invocation itself also triggers the guarded
  rollback (a definite tmux rejection does not, so a racing foreign session is never
  killed). `MultiplexerError::Cancelled` replaces the string-matched failure. Test
  `cancellation_after_create_still_rolls_back_the_session_and_server` fails without it.
- Gates: full `scripts/check` passed on Linux. macOS/native lanes were not run; this
  changes no native code.
