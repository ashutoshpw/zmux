# Phased repair and verification

Implementation authorized2026-10-16. Keep implementations original MIT and use normal hooks. Each phase
ends in one cohesive working commit or a few related commits; separately record a
failing behavioral regression before its fix when repairing production behavior.
No full-parity label follows from this bounded repair.

## Phase 1: establish the current macOS reproduction

Priority correction: the user reports mostly-idle high CPU, including without
Claude. Start with idle attribution; the synthetic active-screen bottleneck is
not an established explanation of this case. The observed missed completion is
local, and the user explicitly requires Phase3 SSH integration in this same
session. Both local and SSH notification journeys gate the delivered build.

Owners: existing resource sampler/native idle workload runner, macOS development
workflow, Rust session/bridge counters and native presentation instrumentation.

- Exercise the exact supplied product source with one/eight idle terminals, two
  visible terminals, colored and Unicode TUI updates, hidden producers and one
  authenticated SSH session. Retain font, geometry, display scale, app focus,
  output/update rate, power context, source SHA and process identities.
- Add bounded opt-in counters/timing: PTY bytes; exported frames/cells/runs;
  per-surface version; JSON bytes/serialize/decode time; native row draws; chrome
  updates; coordinator wakeups; queue pressure. Capture CPU stack samples during
  the same active phases. Exclude diagnostic overhead when presenting budgets.
- Prefer reusable production timing boundaries and a manual Actions profiling
  lane. Never inject arbitrary remote commands through telemetry, alter the user's
  running cmux, or equate virtual-Mac CPU values with physical-Mac values.
- Capture provider integration status, Claude version, current notification
  permission and event-to-inbox/native scheduling dispositions without recording
  prompts/transcripts/secrets. Check ordinary local Claude versus SSH separately.
- Capture distinct settled phases: fresh blank idle shell; two idle shells; two
  idle shells plus idle SSH; local Claude waiting at its prompt; and the same
  phases with zmux inactive/minimized. Record whether the screenshot-producing
  Claude command was working. A running Claude process does not establish ongoing
  output, and cannot explain the user's no-Claude observation by itself.
- Correlate flat PTY byte counters with session/desktop revisions, exports,
  decoding, redraws, chrome updates and wakeups. Sample app/helpers separately;
  distinguish Rust polling/publication, Swift decode, native layout/drawing and
  accessibility from other subsystems before selecting a fix.
- Repair the measured idle owner in a cohesive slice with a behavioral regression
  where applicable. Use dirty reasons/revisions and bounded event/deadline wakeups
  as justified by the profile. Preserve attention, browser, cleanup and timeout
  progress; increasing sleep intervals alone is not a repair for redundant work.

Acceptance: identify dominant active/idle stack costs for the current packaged
build, confirm whether the high-CPU report was idle or updating, and freeze an
explicit baseline/target for each workload. Pair builds on the same named host;
use repeated samples and report variability. Existing idle-only evidence is insufficient.
Do not close the idle issue using the Linux active-output probe or older native
idle build. Repeat current-build idle phases after the measured-owner repair.

## Phase 2: connect provider completion to the shared attention owner

Owners: `apps/cli/src/hooks.rs`, its plugin manifest/runner, Rust attention and
session routing, native notification settings/presentation, shell bootstrap and
the bundle builder. Keep provider lifecycle distinct from message-consumption state.

- Add a typed bounded provider-event projection for supported Claude Stop,
  Notification/permission attention, failure and SessionEnd semantics. Verify event
  fields against the actual supported provider version; do not require a field
  that the provider does not emit. Maintain a zmux-owned turn generation where
  necessary, and deduplicate event IDs/turn completion within bounded retention.
- Bind notices to the original runtime/namespace/surface/session/provider lifetime.
  Reject stale/replayed/cross-surface events; account for subagents, interrupted
  turns and stop-hook recursion without inventing provider success.
- Publish completion through the existing attention mutation/OS desired-state
  path. A provider Stop is sufficient for completion attention, never Ack,
  ReadTurn, automatic continuation or proof that an injected message was read.
- Make ordinary interactive `claude` work in supported zmux shells. Add owned
  zsh/bash startup integration and a recursion-safe wrapper that resolves the real
  executable and preserves its argument boundaries, user startup configuration,
  user/project/managed hooks and exit behavior. Shell coverage must be explicit;
  track fish/other supported shells before claiming general parity.
- Prefer per-process supported hook/plugin injection. Do not silently replace
  `~/.claude/settings.json`. Provide idempotent setup/status/uninstall and an
  integration-disabled/passthrough mode. Help/version/noninteractive/provider
  subcommands must preserve their behavior. Remove the notification dependency on
  the message adapter's restrictive launch grammar only through an explicit,
  tested provider compatibility policy; never blindly broaden that adapter.
- Keep hook callbacks finite and nonblocking on failure; make unavailable routing
  visible in zmux diagnostics without corrupting provider stdout or printing input.
- Expose permission/disabled/denied/scheduled/failed dispositions clearly. Offer
  Enable System Notifications from foreground UI; retain inbox/badge attention if
  OS delivery is unavailable. Never label scheduling acceptance as a shown banner.

Acceptance: actual plain Claude finishes a turn with its process still running;
background app gets one unread attention item and one authorized native scheduling
request, clicking targets the originating live terminal; foreground behavior follows
the documented policy. Denied permission still produces inbox/badge attention.
Duplicate/subagent/late events cannot notify another session. Existing user hooks
and the independent agent-message delivery guarantees remain intact.

## Phase 3: deliver completion attention through managed SSH

Owners: managed connection/session runtime and provider-event ingress from Phase2.

- Design the smallest versioned, session-scoped remote event channel, using either
  an explicitly bounded terminal protocol extension or a reviewed authenticated
  relay. Plain OSC already accepts terminal-generated notifications; do not promote
  it into authority over other sessions or arbitrary local actions.
- Bootstrap owned remote provider integration through the managed connection only
  with visible setup/status/opt-out. Preserve remote hooks, home configuration,
  SSH authentication, host-key policy and remote startup behavior. Never forward
  the local unrestricted Unix socket or assume exporting local env configures SSH.
- Bind event lifetime to the original remote session/connection generation; handle
  disconnect/reconnect, terminal moves/close, local tmux and nested sessions.

Acceptance: a real authenticated remote Claude turn produces attention on the
correct local surface while zmux is unfocused. Closed/replaced/disconnected routes
cannot target another workspace. Do not mark this phase passed with a local stub.

## Phase 4: remove avoidable native drawing and chrome work

Owners: `TerminalView.swift`, `WorkspaceViews.swift`, `App.swift`,
`ApplicationCoordinator.swift` and shared presentation keys.

- Cache font metrics and normal/bold/italic font/attribute variants with correct
  invalidation for font/style/display changes. Avoid repeated NSString measurement
  and NSFontManager conversion per draw run.
- Add authoritative visible damage/frame versions. Invalidate/draw changed rows,
  cursor, selection and IME regions; do not redraw for bytes that alter no pixels.
  Skip unaffected terminal accessibility/content updates while preserving VoiceOver.
- Gate workspace rows, headers, badges, tab attention and other chrome by their
  own inputs. Preserve existing keyed layout reconstruction and status-menu guards.
- Skip pixel presentation for hidden/minimized/occluded windows, retaining terminal
  parsing, bounded history, semantic events and delivery deadlines. Catch up with
  a coherent full frame when visible again.

Acceptance: cursor-only updates avoid full-screen traversal/drawing; one changed
pane leaves another unchanged pane/chrome untouched. Native selection/copy/Unicode,
wide characters/IME, scrolling, resizing, attention and focus regressions pass.
Measure frame time and CPU before/after; lowering update rate alone is not success.

## Phase 5: separate terminal data from desktop control snapshots

Owners: terminal adapter/session publication, desktop runtime snapshot API,
native bridge C ABI/header and Swift `NativeBridge`/model decoding.

- Add per-session frame revisions and changed-row/cursor/metadata payloads. Keep
  workspace, attention, browser and window revisions independent of terminal pixels.
- Retain full snapshots for initial attach, resize, scroll/history, recovery and
  local CLI inspection. A native consumer needs only changed visible frames.
- Use a bounded typed row/frame transport or compact delta format; intern repeated
  styles. Do not simply replace JSON with a binary encoding of the same redundant
  whole-desktop data. Preserve ABI ownership, exact lengths, queue bounds and
  release rules. Reject obsolete namespace/session revisions after close/restore.
- Preserve Unicode cell origins with cached row/glyph layout or positioned-cell
  records. Do not fix Unicode run amplification by concatenating glyphs with wrong
  advance widths. Keep copy text and render geometry consistent.
- Coalesce presentation notifications and make coordinator wakeups demand/deadline
  driven where measurements justify it. Keep ordered PTY parsing, causal replies,
  cancellation, timeout/recovery and notification processing independent of repaint.

Acceptance: an unchanged second pane contributes no full frame to repeated first-
pane updates; a single-cell update has bounded payload independent of full-screen
text/run count. A consumer recovering from missed deltas receives an exact coherent
full baseline. CLI read-screen/selection, hidden output and shutdown still work.
Repeat the retained synthetic probe and native paired workloads after each slice.

## Phase 6: renderer decision, acceptance and artifact

- Re-profile after Phases4/5. If glyph rendering remains dominant, design an
  original retained GPU renderer or integrate an appropriately licensed rendering
  component with notices. Merely enabling the optional Ghostty VT parser leaves
  the current JSON/AppKit pipeline in place and is not this renderer change.
- Compare CPU, GPU, memory/footprint, energy, frame/input latency, Unicode/IME and
  stability. Do not promise that moving work to GPU reduces every resource.
- Run focused Rust/ABI/native checks and `scripts/check`; preserve normal hooks.
  Execute current-SHA macOS Actions validation and archive checks. Do not reinstate
  the explicitly deferred native Save-dialog Accessibility gate.
- Check actual authorized banners with a user-session acceptance receipt. Hosted
  scheduling tests cannot establish delivery under the user's Focus settings.
- Publish a clearly scoped macOS development artifact through GitHub Actions with
  source SHA, relevant performance/notification receipts and remaining gates.

Acceptance for this report: reduction of the reproduced mostly-idle CPU problem,
local plain-Claude background completion accepted, matched updating workloads
checked, correctness/stability preserved and installable artifact tied to the
tested executable. Phase3 managed-SSH notification acceptance is also required
in this session, before the repair build is handed off. Do not defer it.
These repairs alone do not close full cmux parity.

## Nested remote terminal repair

The authenticated SSH acceptance fixture also enters a private tmux server. Direct
OSC777 writes were swallowed by its inner PTY (red log
`/tmp/zmux-tmux-notification-red.log`). A session-owned 0600 FIFO in the 0700
bootstrap root carries only stop/failure/attention tags to a blocked relay that
retains the original TTY descriptor. Shell children inherit neither the TTY relay
FD nor the outer owner's keepalive FIFO writer. Session close releases the writer,
removes exact hook/FIFO files and lets the reader reach EOF. Hook invocations keep
their existing three-second provider timeout; stale inherited wrappers bypass
missing plugins. This does not modify user tmux or Claude configuration.

One extra sleeping process per integrated shell is the lifecycle cost. Plain and
nested authenticated SSH both pass exact-surface inbox attention, completion while
provider remains alive and hook cleanup; no real Claude API or macOS banner is
claimed. A configured OpenSSH RemoteCommand bypasses bootstrap. An existing bounded
process runner is shared for ssh -G configuration resolution on the launch worker
(5s, 64KiB per stream, private group cleanup), without blocking terminal_spec/UI.
Config probe failure retains ordinary SSH behavior and reports integration disabled.

## Darwin owner-only hangup and legacy signal reset

The real macOS probe leaves the remote session leader orphaned and waiting on its
foreground shell/provider after the local SSH client exits, while nested tmux
cleans up. Relay group-HUP traps alone do not cover that owner-only disconnect.
The owner now launches the interactive child asynchronously with explicit original
stdin, closed relay/keepalive descriptors and an interruptible wait builtin.

Bash3/4 irrevocably ignore INT/QUIT in an async subshell, even after trap reset.
On those owners only, `/usr/bin/perl -f` sets those two dispositions to default
and immediately execs the original shell argv. It stays out of the resident process
tree. Modern Bash and dash need no interpreter helper. A missing legacy helper
leaves an ordinary usable shell with an explicit integration-unavailable message.
Private helper startup suppresses Perl module autoload and restores original
PERL5OPT/PERL5LIB set/unset values before exec; values remain environment data,
never command arguments. User startup/provider configuration remains unchanged.

The real-PTY lifecycle test covers owner-only and group HUP with an independently
held Rust lease, ordinary Ctrl-C and hostile private-helper autoload preferences.
A private GNU Bash3.2 compatibility build reproduces the owner-HUP regression and
passes the repair. No GNU code/binary is imported or shipped. Optional test-only
ZMUX_TEST_BASH selects an independently installed compatibility Bash; CI uses the
platform's actual /bin/bash. The real macOS plain/nested disconnect probe and
unchanged10s packaged cleanup assertion remain required product gates.

## Native upload error visibility

Broad macOS build38941972167 passes native terminal/browser/keyboard/reorder stages,
then the existing combined remote clipboard test times out after successful image
and file uploads. Compact MainHeaderView permanently hides detailLabel, including
refused-directory error text. Update now reveals detail only in the error state;
ordinary chrome stays compact. The existing real native paste/refusal assertion
is the regression owner; no test is bypassed or rewritten to accept hidden errors.
