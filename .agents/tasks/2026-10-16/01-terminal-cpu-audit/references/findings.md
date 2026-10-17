# CPU and Claude completion findings

2026-10-16. Investigated zmux `5527ba0bdd62afd3482d66c1d9a905082462d213`.
Product paths examined are unchanged from the supplied `38bfc72` development app.
Read-only reference: cmux `3ba653b9e5afb11d0bc578031142950d6136697e`.
No reference implementation or assets imported. Runtime code was unchanged at
this initial audit; subsequent implementation receipts are recorded in status.md
and native-frame-delivery.md in this task.

## What the screenshot establishes

User clarification after the initial audit: terminals were mostly idle, with high
CPU also observed without Claude. The screenshot was captured with only one active
Claude inside zmux. The missed completion notification was from local Claude on
the Mac, not SSH. Current-build macOS idle attribution is therefore first priority.
The active-screen findings below remain valid but are not the established
explanation of this mostly-idle CPU report. Local completion ingress is the
relevant observed notification defect. The user subsequently required SSH
notification integration in the same session, so it remains a delivery gate.

Main-app CPU: cmux10.9%, zmux31.5% (about2.9 times). Main-app RAM:
cmux245MiB, zmux83MiB. The other shown scopes also use less RAM in zmux.
The screenshot does not establish a zmux memory regression, leak, sustained CPU
baseline, or a matched-workload comparison. Number of sessions is not their update
rate, screen size, text-run count, or terminal workload. App/child totals must remain
separate. In conventional macOS process CPU reporting100% means one logical CPU.

## Confirmed CPU mechanisms

1. **Whole-screen extraction on changed session publication.**
   `crates/session-runtime/src/lib.rs:1586` calls the terminal owner viewport
   exporter when publishing. The default owner delegates to vt100 in
   `crates/session-runtime/src/terminal.rs:52`. Adapter `lib.rs:348` traverses every
   row/cell and allocates drawing runs; `:409` additionally builds complete plain
   contents and copy metadata. Session frame publication is limited by16ms while
   output is active. Default vt100 does NOT export a viewport for every4096-byte
   read: the staged optional Ghostty adapter differs and is not this build.
2. **Full desktop/frame JSON crosses the bridge for every changed revision.**
   `crates/native-bridge/src/lib.rs:812` serializes the complete public snapshot.
   `crates/desktop-runtime/src/lib.rs:2643` includes every selected terminal from
   every selected workspace/window. A single changed session therefore resends
   other visible frames. `control_frames` is explicitly skipped by Serde; hidden
   terminal grids are NOT duplicated into this JSON. They still parse/publish on
   their own workers and can trigger desktop revisions.
   `NativeBridge.swift:123` copies and decodes it using `convertFromSnakeCase`.
   The actor keeps this work OFF the AppKit main actor; off-main work still costs
   app CPU. `ApplicationCoordinator.swift:216` polls about every32ms plus work.
3. **Unicode run amplification.** Adapter `lib.rs:379` merges only ASCII text
   into same-style runs; adjacent non-ASCII cells remain individual runs with
   repeated style fields. This preserves grid origins but greatly amplifies
   extraction, JSON, decoding and AppKit draw-call counts. Fixing it must preserve
   cell placement, combining characters, wide glyphs, selection and copy.
4. **Full native redraw with repeated text/font work.**
   `TerminalView.swift:104` treats any bytes-read change as needing display;
   `:113` traverses all lines/runs without a dirty-row selection. `:46` measures
   an NSString every time cellWidth is accessed (twice per run at `:124`). `:131`
   converts fonts and builds attributes on each run, each draw. Even escape output
   that changes no visible cell can trigger a full redraw.
5. **Chrome updates coupled to terminal output.**
   `ApplicationCoordinator.swift:457` reconciles the whole desktop on each new
   snapshot; `App.swift:699` calls workspace-row/header/attention updates.
   `WorkspaceViews.swift:200` unconditionally rewrites row presentation and marks
   it for display. Layout/sidebar reconstruction already has keys/revision guards;
   it is inaccurate to claim the entire hierarchy is rebuilt every frame. Status
   menu content also already has state guards. Remaining presentation updates
   still happen even when only terminal pixels changed, including while hidden.
6. **Periodic background work.** The desktop coordinator wakes at16ms and scans
   sessions and several model projections; native polling uses32ms. Session PTY
   reads themselves use descriptor polling and a250ms idle bound, NOT a busy-read
   loop. Existing unchanged-revision handling avoids JSON serialization while
   fully idle. Timers are a contributor to wakeups, not proof of31.5% idle CPU.

cmux's reference `Sources/GhosttyTerminalView.swift:1853` coalesces engine wakeups;
`:5685` uses the engine's CAMetalLayer; `:7790` delegates rendering to Ghostty.
zmux currently uses a Rust vt100 backend and Swift/AppKit text drawing, not an
egui/eframe terminal renderer. Rust does not eliminate full-grid allocation,
serialization, decoding, text measurement, or unnecessary drawing. Swapping only
the parser for Ghostty while retaining this exporter/bridge/drawer cannot remove
all these costs. CPU/GPU/memory effects of any renderer replacement need measurement.

## Executed bounded backend measurement

Built the actual default Rust native bridge with
`cargo build --locked --release -p zmux-native-bridge` on Linux x86_64. Probe uses
the production C ABI, two actual PTY children, two40x120 frames,30Hz32ms consumer
polling,3s settling and6s measurement per case. After initially painting the grid,
each producer changes one ASCII digit at30Hz using seven-byte cursor/text writes.
Fixture lifetime is45s; each desktop is destroyed through the real shared owner.
All four destroy calls return0; each captured fixture PID is absent afterward.

| Filled screen | Samples/s | JSON bytes/frame | JSON MB/s | Rust-worker CPU % | FFI/copy CPU % |
| --- | ---: | ---: | ---: | ---: | ---: |
| Idle ASCII | 0.17 | 40,276 | 0.0067 | 0.92 | 0.18 |
| ASCII | 30.40 | 40,272 | 1.22 | 3.70 | 0.90 |
| Color, runs of8 cells | 30.79 | 256,610 | 7.90 | 4.35 | 2.85 |
| Unicode, repeated Greek lambda | 29.81 | 1,678,430 | 50.03 | 6.98 | 15.23 |

Decimal MB. Active cases read only2443/2471/2457 PTY bytes between first/last
sample, respectively. Last snapshots contain80/1282/9600 drawing runs. Unicode is
an intentionally run-heavy synthetic stress case, not a claim about the user's
screen. The idle row includes one baseline fetch rather than spontaneous updates.

Worker CPU is process CPU minus observer-thread CPU, excluding PTY child CPU.
FFI/copy time includes Rust serialization plus Python caller allocation/copy/release
overhead, so is not a pure Rust stack attribution. Observer reads revision only
during measurement; full JSON decoding/counts happen outside the timed interval.
No Swift decoder, AppKit rendering, GPU work, actual Claude or authenticated SSH
is included. Host is shared/busy; these are one-pass diagnostic values, not Mac
budgets or a direct numerical comparison to cmux. An initial observer that decoded
each full frame in Python bottlenecked Unicode consumption; its CPU is explicitly
not a product metric. The revised probe removes that confounder.

Evidence retained under `target/agent-validation/terminal-cpu-audit/`:
`fixture.py`, `probe.py`, `results.json`, `probe-no-frame-decode.py`,
`results-no-frame-decode.json`, and four generated snapshot JSON files.
These generated paths are ignored/local. No prompts, credentials or user terminal
contents were collected. The final result/probe/fixture hashes are in `evidence-hashes.txt`.

Earlier retained native idle test at `aac6677`, run38829001880, observed about0.47%
CPU for one idle shell and0.41% for eight over120s on a hosted ARM VirtualMac.
Its receipt was reread locally, not rerun. That older build/workload does not
exclude a current-build idle regression and never established active-TUI/SSH cost.

Conclusion: full-frame transport and rendering are reproduced architecture costs
that explain substantial avoidable work during updates. They do not yet explain
the clarified mostly-idle report. Exact attribution of31.5% remains a macOS
stack-profile gate. Current Linux idle worker plus FFI/copy CPU was about1.1%
with no PTY bytes after settling. Correlate flat PTY counters with snapshot churn,
native UI work and wakeups rather than assuming the active-output cause applies.

## Confirmed Claude notification gap

The desired event is **a Claude turn finishing while the process stays running**,
not terminal process exit. zmux has a generic notification pipeline but lacks the
equivalent automatic Claude completion ingress:

- `scripts/build-macos-dev` packages the zmux CLI but no Claude shell wrapper or
  shell bootstrap. Ordinary session launch sets TERM and zmux routing variables;
  it does not install a `claude` shell function or inject a completion plugin.
  cmux's shell integration installs a Claude wrapper; its wrapper adds lifecycle
  Stop/Notification hooks. Merely starting an arbitrary PTY program cannot produce
  a reliable semantic completion event.
- `apps/cli/src/hooks.rs:95` requires explicit private plugin installation and
  `hooks claude run --directory ... --surface UUID`; plain `claude` does not use it.
  Install currently accepts only Claude Code2.1.301 and an unchanged provider binary.
- Even this explicit adapter has no notification projection. `hooks.rs:627`
  routes Stop through the default no-op; the file contains no notification action.
  `hooks/plugin.rs:356` registers Stop but no Notification event. This was built
  for safe agent-message injection/leases, not completion attention. Its guarantee
  that Stop cannot Ack/ReadTurn is correct and must remain intact: a separate
  completion notification does not acknowledge agent-message delivery.
- `crates/terminal-adapter/src/lib.rs:185` supports BEL, OSC9 and OSC777. These can
  notify if a provider actually emits them; they cannot substitute for missing
  provider lifecycle wiring. A provider waiting at its prompt need not ring a bell.
- `App.swift:3274` forwards app deactivation; `surface_is_visible` in runtime
  `lib.rs:1816` requires app_active before treating the surface as read. There is
  no source evidence here that background focus suppression alone caused this case.
- Native permission is separately required. `NotificationDelivery.swift:392`
  requests it only from explicit Enable; `:467` requires authorization before
  scheduling. Unknown/denied permission retains inbox attention but no banner.
  User's permission state and Focus settings were not inspected, so do not label
  them the observed root cause or claim a visible-banner reproduction.

If Claude ran through SSH, the current local plugin socket is not a remote event
transport. Launching OpenSSH does not install remote hooks. Existing PTY OSC
ingress can carry an explicit remote notice, but authenticated/session-scoped
remote completion ingress still needs implementation and real SSH verification.

This establishes an implementation omission independently of OS permissions.
No real user Claude turn was invoked, no existing provider settings changed, and
no macOS banner was observed on this Linux host. No new behavioral test was added
or passing fixture misrepresented as that missing end-user journey.
