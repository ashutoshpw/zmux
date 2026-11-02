# Status

**Updated:** 2026-11-01

## Implemented slice: Herdr near-black appearance

The shared native palette in `apps/macos/Sources/WorkspaceViews.swift` has been changed to the requested near-black treatment. `WorkspaceRowView` now uses a neutral selected row and label-based text/icon colors, while the blue accent remains on the drag insertion marker.

| Token | Source value | Approximate appearance |
| --- | --- | --- |
| `window` | calibrated white `0.055` | `#0E0E0E` |
| `sidebar` | calibrated white `0.075` | `#131313` |
| `terminal` | RGB `24, 24, 26` | `#18181A` |
| `chrome` | calibrated white `0.085` | `#161616` |
| `separator` | white at alpha `0.085` | subtle translucent divider |
| `selection` | calibrated white `0.14` | `#242424` |
| `selectionBorder` | white at alpha `0.05` | subtle translucent edge |
| `dropInsertion` | RGB `0.24, 0.55, 1.0`, alpha `0.45` | `#3D8CFF` at 45% |
| `subdued` | calibrated white `0.64` | `#A3A3A3` |
| `success` | RGB `0.28, 0.78, 0.46` | `#47C775` |
| `inactive` | calibrated white `0.38` | `#616161` |

The approximate hex values are rounded display equivalents; the source `NSColor` values above are authoritative. Keep selection visually neutral, preserve blue for drop insertion, and retain status colors as semantic indicators. Palette appearance must not be used as evidence of provider activity, connection state, or SSH health.

## Review and verification

- Independent review confirmed that explicit ANSI black is retained by design; this palette update changes native surfaces and selection styling, not explicit terminal ANSI colors.
- Independent local lifecycle review found no blockers; the P3 safe visibility limitation below remains documented.
- Dark-slice hosted smoke checks passed. The full `scripts/check` retry passed on Linux after the lifecycle fixture change; log: `/tmp/zmux-lifecycle-check-r3.log`.
- Packaged-app screenshots for the dark and lifecycle slices were inspected. The screenshot compositor was unavailable, accessibility save was skipped, and no physical Mac verification was performed.

## Dark slice artifact

- Source commit: `b38e7f09341b15567550b7b3fd5a4ac1d92f4813`.
- Workflow run: [41283271682](https://github.com/ashutoshpw/zmux/actions/runs/41283271682).
- Download: [workflow artifact 13075045356](https://github.com/ashutoshpw/zmux/actions/runs/41283271682/artifacts/13075045356).
- Inner ZIP SHA-256: `191f5f43c326f96e736de4441f07465e3920c3bdfe6ac2e6d810a0bb73cf539c`.
- Target: universal macOS 14, arm64 and x86_64.
- Signing: ad hoc. Notarization: none.

## Implemented slice: local lifecycle evidence

Rust now owns a bounded registry for typed local Claude hook evidence, bound to the exact desktop namespace, terminal surface/session, provider session, adapter runtime, and generation. Hook capabilities stay out of snapshots. Same-UID desktop control can submit these claims; this is not provider-process authentication, remote-provider authentication, or provider replay evidence.

The opt-in Claude adapter reports prompt submission, ambiguous attention candidates, confirmed permission/elicitation waits, tool-driven resumption, stopped turns, and session loss. A Stop hook does not establish Idle because parallel Stop hooks may still continue the turn; only a later `idle_prompt` notification is used for Idle. The worker polls freshness even without other snapshot changes. Stale evidence becomes Unknown, later accepted current-generation evidence restores Connected, and disconnected or terminal capabilities cannot be revived. Exact terminal/session replacement and stale event fencing remain in Rust.

`SessionEnd` reasons `clear` and `resume` become Unknown/evidence-lost rather than Disconnected. The existing adapter shutdown and inbox-retirement behavior remains in place, so lifecycle tracking does not resume after those in-session transitions on the same wrapper run.

## Lifecycle verification

- `cargo fmt --all` passed.
- `cargo test -p zmux-desktop-runtime agent_lifecycle` passed the typed hook normalization and controlled-clock freshness/recovery/disconnect test.
- `cargo test -p zmux-desktop-runtime --test agent_lifecycle` passed the control-socket test for exact owner checks, generation/sequence fencing, terminal registration rejection, and evidence-loss replacement.
- `cargo test -p zmux --test provider_hooks` passed all 6 tests, including plugin lifecycle and nonblocking hook ingress.
- These are local Linux source checks. The full `scripts/check` retry passed after the latest fixture change; output is recorded at `/tmp/zmux-lifecycle-check-r3.log`. The dark-slice artifact above does not verify the lifecycle changes; see the separate lifecycle artifact below. Real Claude provider execution remains unverified. The native sidebar, SSH lifecycle, alerts/sounds, and physical-Mac evidence are also pending.

Independent lifecycle review recorded a **P3 safe visibility limitation**: when Claude self-starts a turn without a `prompt_id`, the lifecycle remains Unknown. Later tool events do not carry enough identity to establish which run they belong to, so they cannot safely create or promote that run. This can under-report activity, but avoids asserting a lifecycle state without run evidence.

## Failed lifecycle hosted attempt (superseded)

- Workflow [41285971536](https://github.com/ashutoshpw/zmux/actions/runs/41285971536) failed on macOS because a new control-socket test placed its Unix-domain socket under the long macOS `TMPDIR`, triggering the socket path / `SUN_LEN` limitation.
- The test fixture was moved to a short path under `/tmp`; the focused test passed after that change.
- The full local `scripts/check` retry passed; output is recorded at `/tmp/zmux-lifecycle-check-r3.log`. This failed hosted attempt was superseded by the successful lifecycle workflow recorded below.

## Lifecycle artifact

- Source commit: `37eaf32e578cae1e6893dfb0715cfb7fe9f89674`.
- Workflow run: [41286844041](https://github.com/ashutoshpw/zmux/actions/runs/41286844041).
- Download: [workflow artifact 13075709288](https://github.com/ashutoshpw/zmux/actions/runs/41286844041/artifacts/13075709288).
- Inner ZIP SHA-256: `bcda46cc70e2698b9aaafd2114288e666822915b6176cebca80c209cf2ec3feb`.
- Target: universal macOS, minimum macOS 14, arm64 and x86_64.
- Signing: ad hoc. Notarization: none.
- Local and hosted checks and packaged smoke passed; packaged screenshot was inspected. Compositor capture was unavailable and accessibility save was skipped.
- Real Claude provider execution and physical Mac verification remain unverified.

## Native alerts and sounds — implementation in progress

The native Alerts pane is implemented as a retained AppKit window controller, with a Zmux-owned `UserDefaults` preference for the master sound switch and independent built-in macOS tone choices for **An agent needs you** and **An agent finished**. Defaults are Ping for needs-user and Glass for completion. Preview plays only that selected local tone and ignores the event master switch; it does not create an alert or change provider configuration. Custom MP3 paths are not supported or read.

`AgentAlertAudioPlayer` is the sole provider-event sound path. It suppresses lifecycle audio while muted, checks the selected `NSSound` resource and the CoreAudio default-output mute property, and reports an unavailable source when detectable. An accepted `NSSound.play()` request does not prove audibility. `NotificationDelivery` now treats requests tagged `agent_alert_kind` as silent at the OS layer (`content.sound = nil`, no `.sound` presentation option), while preserving banner/list presentation. Untagged terminal notifications retain the existing default system sound.

The native event gate consumes a Rust-owned bounded event ring by runtime epoch/cursor, skips the initial and recovery snapshots, advances over stale or dropped history, requires an exact current connected provider/session/run identity including prompt ID, and retains a bounded event-key cache against repeated IDs. `ApplicationCoordinator` silently reconciles after bridge polling recovery or an explicit remote `transport_epoch` change from a validated Hello. Evidence freshness alone can move `Unknown → Connected` without resetting the event cursor, so a newly admitted alert after stale evidence recovery still plays. Rust also creates a tagged attention inbox item from the same admitted event path; `agent_alert_kind` marks the OS request so `NotificationDelivery` keeps the banner/list but omits system sound. The wire carries cursor, adapter sequence, provider, namespace/window/workspace/pane/surface/session identity, provider session, runtime, generation, prompt ID, and `needs_user`/`completed` kind. Adapter sequence is not provider replay evidence. The current local Claude adapter has no provider replay cursor; a validated SSH account/channel handshake does not authenticate the Claude process. Packaged remote acceptance remains pending.

`AgentAlertSmoke` is wired into the existing notification smoke entrypoint. It covers defaults and persisted selections, muted event suppression without audio output, initial/recovery silence, repeated snapshot/event dedupe, stale-generation rejection, the Unknown→Connected freshness interleaving, explicit transport-epoch reconnect reconciliation, overflow reporting, unsupported event silence, flat event wire decoding, and the notification sound discriminator. It does not prove audible playback or that macOS displayed a banner. `git diff --check` passed. The current Linux workspace has no Swift compiler/AppKit SDK, so the Swift smoke and packaged universal build have not been executed here.

## Next implementation hazards — open

These are constraints for the remaining work; they are not implemented or verified by the slices above.

- Serialize an explicit empty lifecycle collection in snapshots. Omitting the field when there are no records can leave consumers unable to distinguish an empty current state from a missing update.
- Filter lifecycle records and updates by the exact window and namespace so an agent cannot appear in another window or namespace.
- Deliver agent lifecycle updates independently of workspace `modelRevision`; an agent state change must not wait for an unrelated workspace model change.
- The current selection command accepts a numeric surface identifier. Add an owner-side action fenced by the full window/namespace/workspace/pane/surface/session identity, and revalidate it at execution so stale identities fail without selecting a replacement surface.
- Keep the Agents list independently scrollable and preserve existing workspace drag-and-drop behavior and hit testing when the sidebar sections coexist.
- Keep the empty-agent state visually neutral; an empty list must not imply an alert, connection failure, or provider activity.

## Pending

- The dark appearance slice has an artifact and hosted verification record, but compositor capture, accessibility save, and physical-Mac proof are unavailable as noted above.
- Verify the lifecycle against a real Claude provider and on a physical Mac; implement and verify the native sidebar, truthful SSH status, exact existing-surface selection, and Alerts preferences, notifications, sounds, and deduplication described in `plan.md`.

The larger live-agent feature remains in progress; agent lifecycle, SSH integration, and alerts/sound behavior remain open.

## Native sidebar identity and focus follow-up

`AgentSidebarList` now reconciles rows by the full stable lifecycle identity, reuses group headers by section, and updates presentation state on existing views. Keyboard focus follows a row across lifecycle/selection changes; if its group becomes collapsed, focus moves to that group header. If the focused row or group disappears, focus moves to the nearest visible row/header or clears when no focus target remains. Disclosure changes preserve focus on the section header. Per-view width constraints are retained and reactivated as rows move between visible and collapsed groups.

Default-titled `Terminal` rows now display their one-based pane/tab context, and the VoiceOver label includes that context, so multiple terminals in the same workspace remain distinguishable. When a window contains duplicate workspace names, the context also includes each workspace's one-based position; named surfaces include the same context in their subtitle and tooltip.

`git diff --check` passed. Native Swift/AppKit compilation and packaged acceptance were unavailable in this Linux environment (`swiftc`, `swift-format`, and `xcodebuild` are absent); this source slice remains unverified on macOS.

## Automatic local Claude lifecycle launch

Compatible interactive Claude commands in an opted-in local PTY route through `hooks claude auto`. The wrapper passes the exact surface, session notice plugin, and original argument vector. The CLI verifies the supported Claude executable and Desktop route before lifecycle setup. If version/options/routing/setup is unsupported before provider start, it launches the request once with terminal notices; after a provider spawn may have started, it never replays the command.

Automatic lifecycle runs with inbox/context delivery disabled. Explicit `hooks claude run` retains its inbox behavior. Neither automatic launch nor presence tracking changes Claude settings or installs a persistent provider plugin. Codex lifecycle remains explicitly unsupported, and SSH lifecycle is not enabled by this local wrapper.

Session cleanup and the automatic launch share an admission lock across plugin publication and runtime-lease acquisition. The uncertain-spawn marker is retained until the provider child identity is confirmed; it is then cleared while the runtime lease remains held. A concurrent cleanup can consequently disable admission but cannot delete the exact leased plugin generation while the provider is running. Cleanup removes the generation after the provider exits and the lease is released. The real-CLI regression is in `apps/cli/tests/provider_hooks.rs`.

The generated `claude` function is installed before user shell startup configuration so a user's own `claude` function can replace it. Calling or defining a shadowing function bypasses automatic lifecycle tracking; users can still invoke the explicit adapter command when appropriate. Fish coverage runs conditionally when `fish` is installed and is recorded in `crates/session-runtime/tests/shell_integration.rs`.

Automatic-launch source verification:

- `cargo fmt --all` passed.
- `cargo test -p zmux --test provider_hooks` passed: 9 passed, 1 ignored helper. The parent regression explicitly invokes the ignored helper in a child test process with a private environment.
- `cargo test -p zmux --test provider_hooks automatic_install_to_runtime_lease_is_atomic_against_real_cleanup_cli -- --nocapture` passed. It uses a real `Desktop`, `ControlServer`, published shell PTY, and the actual cleanup CLI. A cleanup started at installation-record publication hit the bounded lock-busy result without deleting the plugin; cleanup after provider identity confirmation was retained by the active runtime lease; provider exit then removed the generation.
- `cargo test -p zmux-session-runtime --test shell_integration` passed: 7 passed, including conditional fish dispatch (fish was installed in this environment).
- These are local Linux source checks. Full `scripts/check`, native macOS compilation, and packaged-app verification remain pending.

## Lifecycle and terminal notice deduplication

Generic Claude terminal notices remain independent of lifecycle admission. The local and managed SSH hook paths emit attention/completion notices even while lifecycle transport is active, because transport liveness does not establish that the lifecycle owner admitted that exact event. The loopback SSH owner test confirms a permission notice whose hook has no current prompt leaves lifecycle state Unknown with no alert, while the generic notice remains available. StopFailure notices remain available too.

The terminal parser receives arbitrary PTY bytes and records OSC 777 notices as `source: "osc777"`, title, and body. It does not know which process emitted them. Those fields cannot identify Claude or authorize lifecycle state, audio, or deduplication. A typed source would need to come from an out-of-band lifecycle-owner receipt bound to the exact session and event; the current local hook commands are separate and the remote lifecycle stream is one-way, so no such receipt exists.

When both paths produce notices, they remain separate inbox records: the generic terminal notice has no `agent_alert_kind`, while the lifecycle alert is owner-admitted, tagged, and deduplicated by its lifecycle event key. Repeated generic notices with identical content and the same unread terminal owner may coalesce within one second. `NotificationDelivery` requests `.default` and `.sound` for untagged generic notices; tagged lifecycle notifications omit OS sound and use `AgentAlertAudioPlayer` for the selected Alerts sound. Consequently the Alerts master switch cannot mute generic OSC sound requests, and a correlated event can have both the OS default and the Zmux agent sound paths. Actual audibility and displayed banners are unverified on macOS. This duplicate/mute limitation remains open.

Verification:

- `cargo test -p zmux --test provider_hooks automatic_install_to_runtime_lease_is_atomic_against_real_cleanup_cli -- --nocapture` passed. The synthetic provider runs the published hook commands; no-prompt events create no lifecycle alerts, while generic attention/completion and StopFailure notices remain visible. Identical generic notices may coalesce.
- `cargo test -p zmux-remote-runtime --test connection managed_ssh_shell_routes_nonce_bound_opener_requests_from_loopback_sshd -- --nocapture` passed. It verifies a generic attention notice both while the lifecycle stream is active and after lifecycle evidence is lost. The helper's missing `stat` import and the test's stale lock-suppression assertion were corrected.
- `cargo test -p zmux-desktop-runtime retries_a_real_managed_ssh_bootstrap_race_then_reports_session_loss -- --nocapture` passed. The real loopback SSH bootstrap-race case registers the remote runtime, preserves notices while transport is active, confirms no-prompt permission/Stop hooks are rejected by the lifecycle owner, and reports session loss.
- `cargo fmt --all` passed. After isolating the nested test executable and bundled CLI under the child test's private home, `RUST_TEST_THREADS=2 cargo test -p zmux --test provider_hooks` passed: 9 passed, 1 ignored helper. The subsequent `RUST_TEST_THREADS=2 scripts/check` run passed `provider_hooks` and the preceding suites, then failed at `crates/desktop-runtime/tests/native_notifications.rs::plain_claude_completion_reaches_background_attention_while_provider_is_alive` (`desktop state did not settle` at line 287); the full gate is not green. Native macOS compilation, packaged verification, real Claude behavior, production SSH behavior, audible output, and OS banner display remain unverified.

## Current verification update — 2026-11-01

- The full `RUST_TEST_THREADS=2 scripts/check` retry r9 passed on Linux. Log: `/tmp/zmux-agent-sidebar-check-r9.log`. This supersedes the earlier r8 failure as the latest full source-check result; the historical failure note above is retained. This run does not establish native macOS or provider acceptance.
- The packaged sidebar smoke now seeds one Rust-owned `Connected`/`Working` lifecycle through the isolated same-UID control socket, checks the full owner identity, captures `dist/zmux-sidebar-agents.png`, focuses another workspace, clicks the populated row, and verifies exact workspace/pane/surface/session selection without creating or replacing a surface/session. The Python driver validates the socket fixture receipt against the app receipt. Python syntax parsing and `git diff --check` passed locally.
- The populated sidebar smoke has not been compiled or run on macOS. The packaged screenshot and app-selection acceptance are therefore pending; the Linux source check does not cover them.
- The current sound limitation remains: generic untagged OSC terminal notices can request the OS default sound even when the Alerts master switch is muted. A correlated event can therefore produce both the OS default and the selected Zmux agent sound. Actual audibility and banner presentation remain unverified on macOS.
- Codex lifecycle remains unsupported. Real Claude provider execution remains unverified; local synthetic hook and control-socket fixtures do not establish provider authentication or live Claude behavior.

## Latest verification update — 2026-11-01

- macOS development workflow [41304763573](https://github.com/ashutoshpw/zmux/actions/runs/41304763573) failed in `swift-typecheck` because the result of `remember(...)` was unused. The `_ =` fix has been applied. The dependent sidebar and development jobs were skipped, so this run provides no packaged sidebar or screenshot acceptance.
- The full `scripts/check` retry r10 passed on Linux after the unused-result fix; log: `/tmp/zmux-agent-sidebar-check-r10.log`. This source-check pass does not establish native macOS acceptance.
- Exact local hook receipts for lifecycle admission and silent fallback are now implemented and verified below. Packaged/native acceptance for lifecycle sound mute, audible playback, and banner delivery remains pending; this receipt proof does not establish those behaviors on macOS.

## Exact local hook receipts and silent fallback — 2026-11-01

Local lifecycle hook invocations now carry a Zmux-generated UUID correlation ID. This ID is not a provider event ID and does not supply provider replay guarantees. The bounded Desktop receipt cache returns `AlertCovered` when that invocation newly admits an alert or the same run's alert was already covered; lifecycle events that are accepted without producing an alert return `NotHandled`. Reuse of a hook UUID for a different event is rejected. Notification and Stop use one receipt-aware callback; missing, uncertain, rejected, or `NotHandled` outcomes enqueue a visual-only inbox fallback. An `AlertCovered` receipt avoids that fallback.

The generic notice path maps `osc777-inbox` notices to `desktop: false`. The in-app notification row remains present with delivery marked `Suppressed` (`disabled`), and the native notification snapshot contains no matching desired request. The real CLI/Desktop/control-socket integration test covers no-prompt Notification/Stop fallback and checks those owner-visible properties. Notification fallback classification uses only validated matchers: permission/elicitation prompts map to attention, `idle_prompt` maps to completion, and unrelated or unreadable Notification input creates no fallback. A typed-invalid payload with a known attention subtype keeps its visual fallback. A valid `idle_prompt` with `NotHandled` is suppressed to avoid duplicating the preceding Stop fallback. `StopFailure` remains a separate `osc777` notice and retains its existing visibility assertion; it is not a receipt fallback.

Local verification:

- `cargo fmt --all` passed.
- `cargo test -p zmux-desktop-runtime --test agent_lifecycle` passed: 2 passed, including `exact_hook_receipts_cover_new_and_same_run_alerts_and_reject_reuse` through the real Desktop/ControlServer boundary.
- `cargo test -p zmux --test provider_hooks` passed: 9 passed, 1 ignored helper. The parent test executes the helper in a private child process and confirms the Notification/Stop fallback policy through the real provider hook route.
- `cargo test -p zmux --test provider_hooks automatic_install_to_runtime_lease_is_atomic_against_real_cleanup_cli -- --nocapture` passed separately: 1 passed.
- `cargo test -p zmux-desktop-runtime remote_reconnect_changes_transport_epoch_and_requires_a_fresh_prompt -- --nocapture` passed: 1 passed after the heartbeat fixture carried its binding generation.
- `/root/remote` reports `cargo test -p zmux-desktop-runtime remote_agent_lifecycle::tests::retries_a_real_managed_ssh_bootstrap_race_then_reports_session_loss -- --nocapture` passed: 1 passed. Its loopback SSH path covers no-prompt silent fallback, newly admitted and already-covered receipts, `NotHandled` Stop fallback, stale-generation receipt rejection, and no fallback after covered receipts.

## Late Stop ordering and matcher-specific fallback — 2026-11-01

A hook-correlated `TurnStopped::Unknown` arriving after `idle_prompt` for the same completed local prompt now consumes its sequence and returns `HandledNoAlert` without changing `Idle` or adding another fallback. Replaying the exact hook returns the retained receipt. Explicit Stop continuation evidence is kept distinct from generic `Resumed`: local `stop_hook_active: true` normalizes to `StopContinued`, while Stop background work remains `TurnStopped::Working`. For the exact current completed prompt, either explicit continuation event updates activity to `Working` and returns `HandledNoAlert`, preventing a false finished fallback. Local Notification error fallback is restricted to `permission_prompt`, `elicitation_dialog`, and `elicitation_url_dialog`; `idle_prompt` remains `stop_silent`. Syntactically unreadable Notification input has no validated subtype, and unrelated types such as `auth_success` do not create fallback alerts. A parseable but typed-invalid permission prompt still gets the visual fallback.

Focused local verification:

- `cargo fmt --all -- --check` passed.
- `git diff --check` passed.
- `cargo test -p zmux-desktop-runtime --test agent_lifecycle exact_hook_receipts_cover_new_and_same_run_alerts_and_reject_reuse -- --nocapture` passed: 1 test through the real Desktop/ControlServer owner boundary, covering late unknown Stop, explicit Stop continuation/background-work receipts, replay, retained Working/Idle state, and no extra alerts.
- `cargo test -p zmux-desktop-runtime claude_hook_payloads_normalize_only_explicit_lifecycle_events -- --nocapture` passed: 1 unit test confirms Stop with `stop_hook_active: true` maps to `StopContinued`, separately from generic `Resumed`.
- `cargo test -p zmux --test provider_hooks automatic_install_to_runtime_lease_is_atomic_against_real_cleanup_cli -- --nocapture` passed: 1 real-provider route integration test covering unreadable input, `auth_success`, typed-invalid permission, receipt-covered permission/completion, and a late explicit continuation Stop that retains Working without another fallback.
- The full `scripts/check` was not run for this focused follow-up; it remains a parent-owned gate. These tests are local Linux source checks and do not establish macOS notification delivery or real Claude-provider behavior.

These are local Linux source checks using synthetic provider and loopback SSH fixtures. They do not authenticate the Claude process, establish provider replay guarantees, or verify macOS banner delivery or audible playback. The broader app build and artifact acceptance remain pending as recorded above.

## Remote receipt fencing and Stop ordering — 2026-11-01

When a receipt-required remote hook cannot be retained because its endpoint is unavailable, its deadline has expired, or the pending receipt queue is full, the stream now emits a terminal generation-bound `Gap` and exits. The owner therefore fences the old transport state as `Unknown`; the dropped hook does not enter activity or alert admission. A late remote `Stop` for a prompt already completed by `idle_prompt` returns `HandledNoAlert` without regressing `Idle` only when its admitted event is `TurnStopped::Unknown`. Explicit `stop_hook_active: true` (`StopContinued`) and background-task (`TurnStopped::Working`) evidence remains admitted as Working, with `HandledNoAlert` preventing the generic "finished" fallback. This matches the local owner's Unknown-only completion guard while preserving explicit continuation evidence.

Remote hook commands carry fixed event-name arguments. Matcher-specific Notification commands carry only the trusted attention matchers (`permission_prompt`, `elicitation_dialog`, and `elicitation_url_dialog`) and act as fallback-only for unreadable or oversized input. Parseable Notification payloads stay on the generic lifecycle command; unrelated values such as `auth_success` do not receive an attention fallback. The helper writes the silent `osc777-inbox` fallback before reporting loss to the stream.

The remote hook receipt remains at-least-once. If a `Stop` receipt is lost before the later `idle_prompt`, its static `osc777-inbox` visual fallback may appear alongside the single owner completion alert. The Stop itself adds no lifecycle alert event, so it does not request a second Zmux lifecycle sound. This is source/loopback evidence; it does not establish actual macOS sound or banner behavior.

Focused remote verification:

- `cargo test -p zmux-remote-runtime --test connection required_remote_receipt_failures_emit_a_generation_gap -- --nocapture` passed. The production stream helper emitted a Gap without a hook frame for missing and expired endpoints and the 65th pending receipt; the expired endpoint received no negative receipt.
- `cargo test -p zmux-remote-runtime --test connection managed_ssh_shell_routes_nonce_bound_opener_requests_from_loopback_sshd -- --nocapture` passed. The live loopback SSH fixture retains and forwards the 64 queued exact receipts, then continues to the existing FIFO-loss Gap check.
- `cargo test -p zmux-remote-runtime --test connection configured_remote_hooks_fallback_for_unreadable_payloads_and_ignore_other_notifications -- --nocapture` passed. It extracts and executes the production remote hook commands: malformed Stop and oversized matched Notification input write their silent fallback before Gap; `auth_success` produces one generic lifecycle frame with no attention fallback, and a valid permission Notification is not ingested again by its matcher command.
- `cargo test -p zmux-desktop-runtime remote_agent_lifecycle::tests::remote_owner_keeps_completed_idle_after_late_stop -- --nocapture` passed. It verifies a Working run becomes Unknown with no alert on transport loss, then checks late Unknown Stop preserves completed Idle while active/background-task Stops remain admitted as Working with `HandledNoAlert` and no extra alert.
- `cargo test -p zmux-desktop-runtime remote_agent_lifecycle::tests::retries_a_real_managed_ssh_bootstrap_race_then_reports_session_loss -- --nocapture` passed. Its loopback SSH fixture covers a lost Stop receipt, the silent visual fallback, the later single completion alert, and late Unknown/continued Stop receipts with no further fallback or alert.
- `cargo clippy -p zmux-remote-runtime --all-targets -- -D warnings` passed after boxing `Frame::Hook.event` so every `Frame` value does not embed the large hook payload inline.
- `cargo test -p zmux-remote-runtime --test connection -- --nocapture` passed: 9 tests. `cargo test -p zmux-desktop-runtime remote_agent_lifecycle::tests:: -- --nocapture` passed: 3 tests, including the loopback SSH receipt interleaving.
- `cargo clippy -p zmux-desktop-runtime --all-targets -- -D warnings` passed after `RemoteHookInvocation` grouped the remote sequence and hook ID and both duplicate no-alert branches were combined. No lint allowances were added.
- `cargo test -p zmux-desktop-runtime --test agent_lifecycle` passed: 2 tests. `cargo test -p zmux-desktop-runtime remote_agent_lifecycle::tests:: -- --nocapture` passed: 3 tests, including the loopback SSH receipt interleaving. The Stop-specific local hook normalization test also passed.
- `cargo fmt --all -- --check` and `git diff --check` passed. The full `scripts/check` was not run for this follow-up and remains a parent-owned gate. These are local Linux source/loopback checks, not real Claude-provider or native macOS sound/banner acceptance.

## Latest verification update — 2026-11-01

- The full `RUST_TEST_THREADS=2 scripts/check` retry r16 passed on Linux. Log: `/tmp/zmux-agent-sidebar-check-r16.log`. This source-check pass does not establish hosted macOS or live-provider acceptance.
- macOS development workflow [41305431449](https://github.com/ashutoshpw/zmux/actions/runs/41305431449) passed Swift typecheck but failed in the real `provider_hooks` regression when its control socket exceeded macOS `SUN_LEN` under the long `TMPDIR` path. The test fixture was corrected to use a short `/tmp` path; the failed workflow itself produced no packaged build or sidebar acceptance.
- Local lifecycle receipts now scope fallback behavior: `AlertCovered` avoids a duplicate visual fallback, muted lifecycle events remain silent, and synthetic/loopback checks cover receipt and no-extra-alert paths. This does not prove audible output or native banner behavior.
- Remote receipts remain at-least-once. If a Stop receipt is lost before the later `idle_prompt`, a silent visual fallback can appear alongside the single completion alert; the Stop adds no second lifecycle sound.
- Codex lifecycle remains unsupported. Real Claude-provider execution and hosted macOS build/package acceptance remain pending; synthetic providers, control-socket fixtures, and loopback SSH do not authenticate the Claude process or prove live provider behavior.

## macOS automatic-launch fixture routing follow-up — 2026-11-01

Workflow [41313848699](https://github.com/ashutoshpw/zmux/actions/runs/41313848699), source `ad4f50101bb2d34104aa3fdebfe09e63ceed6824`, failed the provider-hook regression because the synthetic provider started before `claude-auto-*/installation.json` was published. The fixture had bound `ControlServer` at `$HOME/.local/run/zmux/control.sock`, while the macOS CLI's default route is `$HOME/Library/Application Support/zmux/control.sock`. With `XDG_RUNTIME_DIR` removed, the test's macOS CLI could not reach that fixture server, causing automatic launch to take its notice-only fallback path; the observed provider args contained only the session notice plugin. The Actions output did not retain the fallback stderr, so that exact runtime reason was not directly captured.

The fixture now binds the platform's default control-socket path. The publication assertion remains unchanged and its failure diagnostics include the owning PTY output. Local Linux validation: `cargo fmt --all -- --check`, `git diff --check`, and `RUST_TEST_THREADS=2 cargo test -p zmux --test provider_hooks -- --nocapture` passed (10 passed, 1 ignored). The macOS-specific path branch and hosted rerun remain unverified in this workspace; no full `scripts/check` was run for this scoped follow-up.

## Latest verification update — 2026-11-01

- The full `scripts/check` retry r17 passed on Linux. Log: `/tmp/zmux-agent-sidebar-check-r17.log`. This does not establish hosted macOS or packaged app acceptance.
- The hosted `ad4f501` run remains failed pending a macOS rerun. The platform socket-path mismatch is the suspected cause, but Actions did not retain the fallback stderr, so the exact runtime reason was not directly observed. The fixture path correction is in place; no downloadable build or packaged acceptance is claimed from that run.

## macOS test wait hardening — 2026-11-01

The automatic compatibility fallback PTY fixture previously waited for the child to exit before reading PTY output. That order can deadlock when child output fills the PTY queue. The fixture now drains concurrently using a nonblocking, cancellable reader, bounds child exit to 30 seconds and PTY EOF to 5 seconds, and requests child/process-group cleanup on timeout. The reader thread is joined after completion or cancellation; captured output is capped at 1 MiB for diagnostics. The synthetic provider emits 64 KiB before a final marker so the regression exercises PTY backpressure. `portable-pty` normalizes Unix PTY `EIO` after slave closure to EOF; this is not evidence of a Darwin-specific failure.

The cleanup guard records when `try_wait` reaps the PTY leader and then refuses to signal its cached process-group ID, which may already be reusable. Timeout cleanup still signals the live process group before the bounded reap wait. If PTY EOF times out after the child was reaped, reader cancellation and join proceed without signaling that stale group ID.

Heartbeat-aware Hook/Gap test receives now share one 15-second absolute deadline per awaited frame. Heartbeats continue to count as progress without resetting that deadline, and timeout failures include the last heartbeat sequence.

Focused verification on Linux:

- After the cleanup guard adjustment, `RUST_TEST_THREADS=2 cargo test -p zmux --test provider_hooks -- --nocapture` passed: 10 passed, 1 ignored; `cargo test -p zmux-remote-runtime --test connection -- --nocapture` passed: 9 passed; and `cargo test -p zmux-desktop-runtime remote_agent_lifecycle::tests:: -- --nocapture` passed: 3 passed.
- `cargo clippy -p zmux --test provider_hooks -- -D warnings`, `cargo fmt --all -- --check`, and `git diff --check` passed after the cleanup guard adjustment.
- The full `scripts/check` was stopped by the parent and is not part of this follow-up; a hosted macOS rerun was not run. These Linux checks do not establish Darwin PTY behavior.

## Latest verification update — 2026-11-01

- The full `scripts/check` retry r19 passed on Linux with exit status 0. Log: `/tmp/zmux-agent-sidebar-check-r19.log`. This run includes the concurrent PTY drain, bounded child/EOF waits, reaped-process cleanup guard, and absolute heartbeat-frame deadlines.
- Hosted workflow [41316003003](https://github.com/ashutoshpw/zmux/actions/runs/41316003003) was canceled after more than 20 minutes in `Validate Rust core and foreign ABI`. Logs show `automatic_compatibility...` exceeding 60 seconds, but the exact blocked call is unproven; a specific hang cause is not established. A hosted rerun is still needed.
- The canceled workflow skipped the universal app build, packaged sidebar smoke, downloadable artifact, and build receipt. No hosted acceptance or delivered build is claimed.

## Latest verification update — 2026-11-01

- The full `scripts/check` retry r20 passed on Linux with exit status 0. Log: `/tmp/zmux-agent-sidebar-check-r20.log`.
- Hosted workflow [41320417182](https://github.com/ashutoshpw/zmux/actions/runs/41320417182) failed in `provider_hooks` with `Not a directory (os error 20)` in the symlinked-plugin fixture. Source diagnosis traced this to the Darwin `/tmp` symlink; the fixture now canonicalizes the trusted temporary root and enforces a 98-byte socket-path budget. Independent review found no blocker.
- A macOS hosted rerun is still needed. The failed workflow skipped app packaging and the sidebar smoke, so no hosted acceptance or delivered build is claimed.

## Latest verification update — 2026-11-01

- The full `scripts/check` retry r25 passed on Linux with exit status 0. Log: `/tmp/zmux-agent-sidebar-check-r25.log`. It includes the heartbeat sequence and SSH child-cleanup fixes, the shell `Busy` test handling, and lint fixes.
- Hosted workflow [41321843638](https://github.com/ashutoshpw/zmux/actions/runs/41321843638) failed in the managed SSH lifecycle regression: it observed sequence 9 where the test expected 8, alongside `EPERM` process-cancellation diagnostics. Source review traced the interleaving to the shared heartbeat counter and cleanup of a reaped process group; both were fixed locally.
- A macOS hosted rerun is pending. The failed run skipped universal app packaging, packaged sidebar smoke, artifact upload, and build receipt; no hosted acceptance or delivered build is claimed.

## Remote lifecycle heartbeat and process cleanup follow-up — 2026-11-01

Hosted workflow [41321843638](https://github.com/ashutoshpw/zmux/actions/runs/41321843638) failed the loopback SSH lifecycle test at its exact `sequence == 8` assertion: the idle Hook arrived with sequence 9. The stream helper increments one sequence for both Hooks and five-second Heartbeats, while the test helper consumed Heartbeats without advancing its expected sequence. The test now checks strict monotonic order across each observed Hook and Heartbeat and waits for a real heartbeat before the idle Hook, preserving event order and replay checks without assuming contiguous Hook values.

The same workflow logged `remote lifecycle process cancellation: EPERM` as pre-handshake SSH attempts closed. `ChildOwner::drop` was signaling `child.id()` as a process-group ID even though the polling loop could have reaped that child with `try_wait`; the PID/PGID may then be reused. The hosted log does not identify which extant process group caused `EPERM`. The supervisor now relies on bounded stdout EOF, handshake, heartbeat, and cancellation checks and keeps the child unreaped until cleanup signals its process group and then waits. The guard also skips any group signal after reaping. A regression kills a live group containing a descendant that holds stdout, verifies EOF after teardown, and verifies no second signal after reap.

Focused Linux verification after these changes:

- `RUST_TEST_THREADS=2 cargo test -p zmux-desktop-runtime remote_agent_lifecycle::tests:: -- --nocapture` passed: 4 tests, including the real loopback SSH interleaving and child-group cleanup regression.
- `cargo clippy -p zmux-desktop-runtime --lib -- -D warnings`, `cargo fmt --all -- --check`, and `git diff --check` passed.
- The full `scripts/check` and another hosted run were not performed for this follow-up. Darwin process-group behavior and hosted macOS acceptance remain unverified.

## Remote lifecycle stdin backpressure and bounded reaping — 2026-11-01

Managed SSH lifecycle Activate and Receipt frames now share one nonblocking stdin writer. It retains each serialized frame and partial-write offset until completion, polls child stdin and stdout together, preserves frame order, and applies a fixed two-second write deadline that cancellation checks can interrupt. A stalled or closed remote reader becomes lifecycle loss and enters the existing generation fencing path.

Child cleanup now signals its process group only while the leader remains unreaped, polls `try_wait` for at most 250 ms, then transfers the child to one bounded background reaper. A pre-spawn global reservation caps active plus deferred child ownership at `MAX_TERMINALS`; a stream that defers a reap stops reconnecting, so it cannot accumulate pending children. The background reaper never signals a process group.

Focused Linux verification:

- `RUST_TEST_THREADS=1 cargo test -p zmux-desktop-runtime remote_agent_lifecycle::tests:: -- --nocapture` passed: 5 tests, including the real loopback SSH stalled-stdin test. The test filled the actual child stdin pipe with over 10,000 receipts, observed bounded `Lost`, and verified cancellation disconnects the supervisor while a second pipe remains stalled.
- `cargo clippy -p zmux-desktop-runtime --lib -- -D warnings`, `cargo fmt --all -- --check`, and `git diff --check` passed.
- The full `scripts/check`, hosted macOS workflow, app build, and downloadable artifact were not run for this parent-scoped follow-up. These local loopback checks do not establish Darwin process behavior or hosted acceptance.

The r22 failure was a test-bootstrap race: checking only `is_file()` could observe the stream helper immediately after shell redirection created it, before its here-doc body finished writing. The test now waits for the full generated helper contents, including the bootstrap's trailing newline, before replacing it with the stalled peer. No timing delay was added.

- `RUST_TEST_THREADS=2 cargo test -p zmux-desktop-runtime remote_agent_lifecycle::tests:: -- --nocapture` passed: 5 tests with the two real loopback SSH lifecycle tests running concurrently.
- `RUST_TEST_THREADS=2 cargo test -p zmux-desktop-runtime --lib` passed: 115 tests, including the stalled-stdin test under the full desktop-runtime unit-test load.

## Local Claude wrapper snapshot contention follow-up — 2026-11-01

The r23 failure in `automatic_local_claude_wrapper_dispatches_once_with_notice_plugin_and_original_args` occurred after the test CLI had written its argument report. The failing unwrap was a later `Session::snapshot()`: that API intentionally uses `try_lock`, and returns `SessionError::Busy` while the PTY worker publishes a snapshot. The command's `send_input` had already succeeded, so this did not demonstrate input queue saturation or failed CLI dispatch.

The test now waits for expected PTY output while treating `Busy` as transient and other errors as failures. Its fake CLI writes the argument report to a sibling temporary file and renames it into place, making report existence a completed-write barrier. The readiness helpers use bounded deadlines and scheduler yields, with no fixed sleep.

- `RUST_TEST_THREADS=2 cargo test -p zmux-session-runtime --test shell_integration automatic_local_claude_wrapper_dispatches_once_with_notice_plugin_and_original_args -- --nocapture` passed: 1 test.
- `RUST_TEST_THREADS=2 cargo test -p zmux-session-runtime --test shell_integration -- --nocapture` passed: 8 passed, 1 ignored.
- `cargo clippy -p zmux-session-runtime --test shell_integration -- -D warnings`, `cargo fmt --all -- --check`, and `git diff --check` passed.
- The workspace `scripts/check` rerun is owned by the parent; no workspace check or commit was performed in this follow-up.

## Remote lifecycle one-shot receive lint cleanup — 2026-11-01

Three test helpers used loops around exactly one `recv_timeout`; every match arm already returned, panicked, or broke. They now use direct matches while retaining the absolute deadlines, timeout assertions, binding checks, and unexpected-message failures.

- `RUST_TEST_THREADS=2 cargo test -p zmux-desktop-runtime remote_agent_lifecycle::tests:: -- --nocapture` passed: 5 tests.
- `cargo clippy -p zmux-desktop-runtime --all-targets -- -D warnings`, `cargo fmt --all -- --check`, and `git diff --check` passed. No lint allowances were added.

## Hosted stalled-stdin regression follow-up — 2026-11-01

Hosted workflow [41327050533](https://github.com/ashutoshpw/zmux/actions/runs/41327050533) timed out waiting for `Lost` after 441 admitted receipts. The old 750 ms submission phase established bounded queue admission, but did not establish that the SSH client's or kernel's buffers had filled; the remote SSH channel could accept the data while the child never read stdin. This run therefore did not show a production writer failure.

The stalled peer now sends valid Heartbeats every 100 ms while never reading Receipt frames. A continuous receipt pump records admissions and rejections. The test requires four consecutive Heartbeat intervals with no admission progress and increasing rejection counts before it calls the backpressure sustained. It then waits for `Lost` under one absolute `HANDSHAKE_TIMEOUT`, verifies loss follows the observed live-peer pressure, retains the prior `MAX_PENDING_RECEIPTS` admission lower bound, and asserts prompt loss. The cancellation phase waits for the same sustained pressure, stops the producer, drops the stream promptly, and drains any queued Heartbeats while waiting for sender disconnection.

Focused Linux loopback verification:

- `RUST_TEST_THREADS=2 cargo test -p zmux-desktop-runtime remote_agent_lifecycle::tests::real_managed_ssh_stalled_stdin_has_bounded_loss_and_cancellation -- --nocapture` passed. It admitted 10,187 receipts, observed 3,557,675 rejected calls, confirmed sustained pressure, then received `Lost` 2.27 seconds after production began.
- `RUST_TEST_THREADS=2 cargo test -p zmux-desktop-runtime remote_agent_lifecycle::tests:: -- --nocapture` passed all 5 lifecycle tests, including both loopback SSH tests running concurrently. The stalled case admitted 10,187 receipts, observed 3,598,727 rejections, and received `Lost` after 2.28 seconds. Cancellation drained one Heartbeat and disconnected the sender 110 ms after dropping the stream.
- `cargo clippy -p zmux-desktop-runtime --all-targets -- -D warnings`, `cargo fmt --all -- --check`, and `git diff --check` passed.
- The workspace `scripts/check`, hosted rerun, macOS build, and downloadable artifact were not run for this parent-owned follow-up. The loopback test is local Linux evidence; hosted macOS acceptance remains unverified.

## Latest verification update — 2026-11-01

- The full `RUST_TEST_THREADS=2 scripts/check` retry r26 passed on Linux with exit status 0. Log: `/tmp/zmux-agent-sidebar-check-r26.log`.
- Hosted workflow [41327050533](https://github.com/ashutoshpw/zmux/actions/runs/41327050533) failed in the managed SSH stalled-stdin regression after 441 receipts. Its prior 750 ms receipt burst established queue admission but did not prove SSH/kernel pipe saturation; this was a test-evidence gap, not evidence of a production writer failure. Swift typecheck and scheduler jobs passed; the workflow stopped before universal app packaging and sidebar smoke.
- The regression now uses a continuous receipt pump while the stalled peer sends valid Heartbeats every 100 ms. It requires four heartbeat intervals with no new admissions and increasing rejection counts before classifying pressure as sustained, then waits for `Lost` under one absolute `HANDSHAKE_TIMEOUT`. Focused and concurrent local Linux lifecycle tests passed; detailed commands and measurements are recorded above.
- Independent review found two coverage limits: the producer yields while retrying rejected receipts but does not bound CPU use (observed test runs reached roughly 3.6 million rejections in about 2.3 seconds), and the cancellation phase verifies prompt stream shutdown/sender disconnection but does not itself assert integration-child reap completion. A separate `ChildOwner` test covers process-group termination, leader reaping, stdout closure, and no signaling after reap. The loopback test skips when Python or SSH prerequisites are unavailable.
- A hosted macOS rerun remains pending. This failed run produced no packaged app, sidebar smoke evidence, or downloadable artifact; no hosted acceptance or delivered build is claimed.

## Hosted stalled-stdin deadline-bound correction — 2026-11-01

Hosted workflow [41329593839](https://github.com/ashutoshpw/zmux/actions/runs/41329593839) failed only the test's `<10s` total-from-start assertion. It reported 10,539 accepted receipts, 18,540,147 rejected calls, sustained pressure at 14.640 seconds, `Lost` at 15.887 seconds, and Heartbeats through sequence 89. The 1.246-second interval from the sustained-pressure marker to `Lost` is consistent with the existing two-second per-frame write deadline; the longer macOS time is the time to reach sustained backpressure. This is not evidence of a production write-timeout regression.

The test now captures the `Lost` receive time and bounds it relative to the sustained-pressure confirmation using the existing `CLIENT_FRAME_WRITE_TIMEOUT` (2 seconds), `CHILD_REAP_SYNC_TIMEOUT` (250 ms), and an explicit two-poll scheduling allowance (200 ms). The single 30-second `HANDSHAKE_TIMEOUT` remains absolute from receipt production start. Production timeout values and heartbeat, owner-binding, sequence, admission, and unexpected-message checks are unchanged.

Focused Linux verification:

- `RUST_TEST_THREADS=2 cargo test -p zmux-desktop-runtime remote_agent_lifecycle::tests::real_managed_ssh_stalled_stdin_has_bounded_loss_and_cancellation -- --nocapture` passed. `Lost` arrived 1.240 seconds after sustained pressure, within the derived 2.45-second bound; total time from production start was 2.297 seconds.
- `RUST_TEST_THREADS=2 cargo test -p zmux-desktop-runtime remote_agent_lifecycle::tests:: -- --nocapture` passed all 5 tests with both loopback SSH tests running concurrently. `Lost` followed sustained pressure by 1.109 seconds; cancellation disconnected the sender 110 ms after stream drop.
- `cargo clippy -p zmux-desktop-runtime --all-targets -- -D warnings`, `cargo fmt --all -- --check`, and `git diff --check` passed. Independent read-only review found no blockers.
- The hosted rerun, workspace `scripts/check`, macOS build, and downloadable artifact remain pending. The local checks are Linux loopback evidence and do not establish hosted macOS acceptance.

## Monotonic stalled-pressure confirmation follow-up — 2026-11-01

Review identified that counting four Heartbeat samples alone did not prove four elapsed intervals: buffered frames can be drained back-to-back. This revision first added a 400 ms monotonic no-admission interval while observing valid, strictly sequenced Heartbeats; the subsequent Foundation Ubuntu failure showed that starting that clock at the first Heartbeat sample could lag the pump's first rejection. The following section records moving the timestamp to the receipt pump. No test sleep was added. The 30-second absolute deadline and 2.45-second pressure-relative loss bound remain unchanged; production code and timeout values are unchanged.

Focused Linux verification after the elapsed-time change:

- `RUST_TEST_THREADS=2 cargo test -p zmux-desktop-runtime remote_agent_lifecycle::tests::real_managed_ssh_stalled_stdin_has_bounded_loss_and_cancellation -- --nocapture` passed. Sustained pressure was confirmed 1.322 seconds after receipt production started; `Lost` followed 977 ms later, within the 2.45-second derived bound.
- `RUST_TEST_THREADS=2 cargo test -p zmux-desktop-runtime remote_agent_lifecycle::tests:: -- --nocapture` passed all 5 tests with both loopback SSH tests running concurrently. `Lost` followed sustained pressure by 1.101 seconds; cancellation disconnected the sender 110 ms after stream drop.
- `cargo clippy -p zmux-desktop-runtime --all-targets -- -D warnings`, `cargo fmt --all -- --check`, and `git diff --check` passed.
- Fresh independent read-only review found no blockers: buffered Heartbeats cannot satisfy the 400 ms monotonic interval back-to-back, any admission progress or non-increasing rejection sample resets the observation, and both deadlines remain anchored as intended.
- The parent stopped the workspace `scripts/check`; no hosted rerun was started. These results are local Linux evidence only.

## Latest verification update — 2026-11-01

- The full `RUST_TEST_THREADS=2 scripts/check` retry r28 passed on Linux with exit status 0. Log: `/tmp/zmux-agent-sidebar-check-r28.log`.
- Hosted workflow [41329593839](https://github.com/ashutoshpw/zmux/actions/runs/41329593839) failed the earlier total-from-start `<10s` assertion: 10,539 receipts were accepted, sustained pressure was confirmed at 14.640 seconds, and `Lost` arrived at 15.887 seconds (1.246 seconds after pressure). This reflects the time needed to reach actual backpressure on macOS, not a production write-timeout regression.
- The test now bounds `Lost` relative to sustained-pressure confirmation by the existing two-second client write deadline plus 250 ms child-reap and 200 ms scheduling allowances, while retaining the absolute 30-second deadline from receipt-pump start. Sustained pressure also requires 400 ms of monotonic no-admission progress and increasing rejection counts across valid, sequenced Heartbeats; buffered Heartbeats alone cannot meet the interval. Production timeout values are unchanged.
- Focused and concurrent Linux lifecycle tests passed after these changes. Independent read-only review found no blockers: admission progress or non-increasing rejection counts reset the pressure interval, and the absolute and pressure-relative deadlines remain anchored correctly. Detailed commands and timing evidence are recorded above.
- A hosted macOS rerun is pending. Workflow 41329593839 did not establish packaged app, sidebar smoke, or downloadable-artifact acceptance.

## Foundation Ubuntu pressure-start observation follow-up — 2026-11-01

Foundation workflow [41331773961](https://github.com/ashutoshpw/zmux/actions/runs/41331773961), SHA `52937424102f06bdca974ea51457b2d6c35e6cbc`, passed 114 library tests and failed the stalled-stdin case when `Lost` arrived before the 400 ms pressure marker. At loss it reported 10,716 admitted receipts, 3,531,044 rejected attempts, and Heartbeat sequence 24. The hosted output did not record when the first receipt rejection occurred, so it cannot establish the exact duration of the no-admission run or prove a production timeout failure.

The prior observer started its monotonic clock at the first Heartbeat sample that saw no admissions, which can lag the receipt pump's first rejected send. The pump now timestamps the beginning of each consecutive rejection run and clears that timestamp on the next accepted receipt. On each valid Heartbeat, the observer still requires unchanged accepted count and rising rejections, plus at least 400 ms elapsed from the producer-side rejection timestamp. Buffered Heartbeats cannot manufacture elapsed time, and the test reports the rejection duration at `Lost` if confirmation is missing. The fixed 30-second overall deadline and 2.45-second pressure-relative loss bound are unchanged; production timeout code is unchanged.

Reproduction and focused Linux verification:

- Before moving the timestamp to the receipt pump, `RUST_TEST_THREADS=2 cargo test -p zmux-desktop-runtime --lib` passed all 115 tests; the exact hosted Ubuntu ordering was not reproduced locally.
- After the timestamp change, `RUST_TEST_THREADS=2 cargo test -p zmux-desktop-runtime remote_agent_lifecycle::tests::real_managed_ssh_stalled_stdin_has_bounded_loss_and_cancellation -- --nocapture` passed. The latest run confirmed pressure 741 ms after production began; `Lost` followed 1.567 seconds later, within the 2.45-second bound.
- `RUST_TEST_THREADS=2 cargo test -p zmux-desktop-runtime remote_agent_lifecycle::tests:: -- --nocapture` passed all 5 lifecycle tests. Pressure was confirmed after 831 ms; `Lost` followed 1.429 seconds later, and cancellation disconnected the sender 110 ms after stream drop.
- `cargo clippy -p zmux-desktop-runtime --all-targets -- -D warnings`, `cargo fmt --all -- --check`, and `git diff --check` passed. No `scripts/check` or new hosted workflow was run; the parent had stopped the full check.

## Stable rejection-epoch snapshot and full local check — 2026-11-01

The stalled-stdin regression now stops and joins its receipt pump when `Lost` arrives, then validates an immutable snapshot against the rejection epoch confirmed by valid Heartbeats. It fails if any receipt was admitted after the observed pressure or if the producer's rejection-run timestamp changed, so a stale sustained-pressure marker cannot pass. Cancellation performs the same joined snapshot check. Production timeout behavior and values remain unchanged.

- The focused loopback SSH test passed after this change: sustained pressure was confirmed 941 ms after receipt production began, and `Lost` followed 1.348 seconds later, within the existing 2.45-second pressure-relative bound.
- `RUST_TEST_THREADS=2 scripts/check` passed on Linux with exit status 0 after the stable-snapshot change. Full output: `/tmp/zmux-foundation-stalled-stdin-r29.log`.
- `git diff --check` passed. These are local Linux checks; no new hosted workflow, macOS build, or downloadable artifact was run for this test-only follow-up.

## Latest verification update — 2026-11-01

- The full `RUST_TEST_THREADS=2 scripts/check` retry r30 passed on Linux with exit status 0. Log: `/tmp/zmux-agent-sidebar-check-r30.log`.
- macOS workflow [41331773945](https://github.com/ashutoshpw/zmux/actions/runs/41331773945), source SHA `52937424102f06bdca974ea51457b2d6c35e6cbc`, built and retained a candidate universal archive but failed its isolated terminal-selection metadata compile: `Models.swift:481` could not resolve `NativeAgentAlertEvent`. The selection probe's standalone source list omitted the real declarations in `AgentAlertDelivery.swift` and its `AgentAlertKind` dependency in `AgentAlertPreferences.swift`; both sources are now included in the shared metadata input closure used by completed-history as well. The retained candidate at `/tmp/zmux-41331773945/candidate/zmux-dev-universal-5293742.zip` passed checksum and ZIP integrity checks and reports arm64/x86_64 with macOS 14 minimum, ad-hoc signing, and no notarization. It is not an accepted build: metadata compile failed before packaged runtime/sidebar smoke and screenshot evidence.
- Foundation workflow [41331773961](https://github.com/ashutoshpw/zmux/actions/runs/41331773961) failed the stalled-stdin regression when `Lost` arrived before the 400 ms pressure marker; the preceding section records the run and its diagnostics. The receipt producer now timestamps each rejection run at the first actual rejected send and clears it on successful admission. The test stops and joins the producer on loss and validates an immutable accepted-count/rejection-epoch snapshot against the Heartbeat-confirmed pressure epoch, preventing a stale marker from passing. Independent review covered this producer-side evidence and the bounded deadlines; focused Linux lifecycle tests and the full r30 workspace check passed.
- A hosted rerun remains pending for both the metadata source-list correction and the stabilized pressure evidence. Neither hosted run establishes packaged app/sidebar acceptance or a delivered artifact.

## Metadata compile dependency closure follow-up — 2026-11-01

- The full `RUST_TEST_THREADS=2 scripts/check` retry r31 passed on Linux with exit status 0. Log: `/tmp/zmux-agent-sidebar-check-r31.log`.
- macOS workflow [41334872103](https://github.com/ashutoshpw/zmux/actions/runs/41334872103) built the app candidate, then failed the terminal-selection metadata compile because `NotificationDelivery.swift` could not resolve `AgentAlertAudioPlayer` and `AgentAlertPlaybackResult`. The isolated source list omitted `AgentAlertAudio.swift`; that source is now included. Its production implementation imports `CoreAudio`, which is now part of the shared metadata framework arguments alongside AppKit, WebKit, and UserNotifications.
- The completed-history probe derives its sources from the selection closure and now consumes the same shared framework arguments. Static recursive inventory found no remaining references to app types declared outside the selected source closure; the only lexical candidate is the local generic `Value`. The full app build already links CoreAudio. This Linux host has no `swiftc` or `xcrun`, so local native typechecking is unavailable.
- Workflow 41334872103 produced no accepted build: metadata compilation failed before packaged runtime/sidebar smoke and screenshot evidence. A hosted macOS rerun with the updated source and framework closure remains pending; no delivered artifact or native acceptance is claimed.

## Workspace activity smoke diagnostics — 2026-11-01

- Hosted workflow [41338107926](https://github.com/ashutoshpw/zmux/actions/runs/41338107926) passed terminal selection, then failed the packaged native smoke at the compound workspace activity details assertion. The run did not identify which individual predicate failed, so the product-level root cause remains unknown.
- The native smoke failure path now reports named predicates, bounded rendered text, model status/log summaries, and focus/responder context. All existing acceptance predicates remain enabled and unchanged; this adds diagnostic detail without weakening the smoke.
- `RUST_TEST_THREADS=2 scripts/check` passed locally on Linux with exit status 0 (r32). Log: `/tmp/zmux-agent-sidebar-check-r32.log`. This does not typecheck or run the AppKit smoke.
- A hosted macOS rerun with the diagnostics is pending. The current run does not establish packaged native smoke acceptance or a delivered artifact.

## Diagnostic expression typecheck correction — 2026-11-01

- Hosted workflow [41340991316](https://github.com/ashutoshpw/zmux/actions/runs/41340991316) failed Swift typechecking in the new failure diagnostic: optional chaining made `.map(String.init)` resolve against the unwrapped workspace and pane ID types.
- The diagnostic now formats focused workspace and pane IDs through explicit optional `flatMap` steps. All native smoke acceptance predicates remain unchanged.
- The full `RUST_TEST_THREADS=2 scripts/check` passed locally on Linux with exit status 0 (r33). Log: `/tmp/zmux-agent-sidebar-check-r33.log`. This does not replace the macOS typecheck or packaged smoke.
- A hosted macOS rerun is needed to confirm the corrected diagnostic typechecks and to identify the original workspace-activity failure predicate.

## CLI stdin fixture race follow-up — 2026-11-01

- A scoped fix addresses a race in the CLI stdin fixture using an explicit red interleaving and file-backed input while preserving the production wire contracts. Independent review found no issue with the scoped approach.
- The full `RUST_TEST_THREADS=2 scripts/check` passed locally on Linux with exit status 0 (r34). Log: `/tmp/zmux-agent-sidebar-check-r34.log`.
- Foundation workflow [41343399055](https://github.com/ashutoshpw/zmux/actions/runs/41343399055) passed on macOS but failed on Ubuntu with `BrokenPipe` in a pre-existing CLI test. This is separate from the scoped stdin fixture race fix.
- macOS workflow [41343399053](https://github.com/ashutoshpw/zmux/actions/runs/41343399053) is still running for an earlier source SHA. Its result does not yet verify the latest source.

## Managed SSH snapshot Busy retry and hosted failure reassessment — 2026-11-01

macOS workflow [41341700406](https://github.com/ashutoshpw/zmux/actions/runs/41341700406), source SHA `ff8f8e42d6aa8a28e1acb7353b8b0447f8b21457`, passed Swift typecheck and both Linux scheduler jobs. Its development job failed one Rust library test, `retries_a_real_managed_ssh_bootstrap_race_then_reports_session_loss`, at `inspect SSH terminal session: Busy`; the other 112 library tests passed. `Session::snapshot()` intentionally uses `try_lock`, so this is a transient snapshot-contention case in test observation, not evidence that the managed SSH session entered `Failed`. App packaging, packaged sidebar smoke, and artifact upload were skipped after the core test failure.

The lifecycle tests now use one test-only `session_snapshot_before` helper at all three state-inspection sites. It retries `SessionError::Busy` with `thread::yield_now()` until the caller's existing absolute deadline, propagates other errors, and keeps the `State::Failed` assertions intact. The helper adds no sleep and does not change production behavior or the existing outer bootstrap polling cadence. The existing SSH-notice snapshot helper now uses the same bounded retry.

Hosted-failure reassessment: workflow 41340991316's Swift diagnostic type error is resolved; the subsequent 41341700406 Swift typecheck passed. Workflow 41338107926 still leaves the original workspace-activity smoke predicate unknown: its failure diagnostics were added, but 41341700406 stopped before packaged smoke and therefore does not resolve that product-level question. Earlier stalled-stdin failures at 41329593839 and 41331773961 were separate test-evidence/deadline issues recorded above; this snapshot fix does not establish their macOS rerun acceptance. No hosted workflow in this sequence produced an accepted app artifact.

Local verification after the retry:

- `RUST_TEST_THREADS=2 cargo test -p zmux-desktop-runtime remote_agent_lifecycle::tests::retries_a_real_managed_ssh_bootstrap_race_then_reports_session_loss -- --nocapture` passed: 1 test.
- `RUST_TEST_THREADS=2 cargo test -p zmux-desktop-runtime remote_agent_lifecycle::tests:: -- --nocapture` passed all 5 lifecycle tests. The stalled case confirmed pressure after 877 ms and received `Lost` 1.409 seconds later, within the 2.45-second bound; cancellation disconnected the sender 110 ms after stream drop.
- `RUST_TEST_THREADS=2 scripts/check` passed on Linux with exit status 0. Log: `/tmp/zmux-bootstrap-snapshot-r1.log`. `cargo fmt --all -- --check` and `git diff --check` passed.
- These are Linux source checks. A hosted macOS rerun remains necessary; no macOS package, sidebar acceptance, artifact, commit, or delivery is claimed.

## CLI literal-stdin test fixture race — 2026-11-01

Foundation workflow [41343399055](https://github.com/ashutoshpw/zmux/actions/runs/41343399055), SHA `a886db84dd8ea31973dcc1a5fc72920a3974c4d3`, failed Ubuntu's `send_text_delimiter_preserves_literal_stdin_marker_on_the_wire` at `apps/cli/tests/cli.rs:456` with `BrokenPipe` from the test's write to the spawned CLI stdin. The paired macOS Foundation job passed, including that named test. Previous Foundation workflow [41334872121](https://github.com/ashutoshpw/zmux/actions/runs/41334872121), SHA `1f413138bea5bddf8d0b44062527959ff44a7c9b`, passed the test on both Ubuntu and macOS. The test was introduced in `66ec345` and is unchanged since; the current source has no `apps/cli` diff from that previous green SHA.

The failing write is test-harness I/O, not the CLI's `send-text` request path. The first case passes `send-text -- --stdin`; the production parser keeps `--stdin` literal and does not read stdin, then completes its Unix control request. If the child handles the request and exits before the test parent writes, the pipe reader is closed and the parent's `write_all` returns `BrokenPipe`. The stdin payload is still needed: if delimiter handling regressed and read stdin instead, the test must observe `input from stdin` rather than the expected literal marker. A deterministic child-first red proof using the real CLI and the same framed mock peer confirmed successful literal-marker request handling followed by `BrokenPipe` on a later parent write. The original test passed 200 local repetitions normally and 200 repetitions pinned to one CPU; this does not rule out the hosted interleaving.

The fixture now preloads `input from stdin` into a file and passes that file as the child's stdin before spawn. This keeps both cases' input bytes and the exact wire-payload assertions while removing the post-spawn writer race. No production CLI code changed.

- Focused `send_text_delimiter_preserves_literal_stdin_marker_on_the_wire` passed after the fix; all 13 `apps/cli/tests/cli.rs` tests passed.
- The fixed test passed 200 repetitions pinned to one CPU. `cargo fmt --all -- --check` and `git diff --check` passed.
- The full `scripts/check` was not rerun for this test-fixture-only change. The prior full local check passed all 13 CLI tests before the fixture edit. No hosted rerun or commit was started.
- `scripts/check-changed.mjs` and an automated `autoreview` tool are unavailable in this checkout/session; focused package verification and source review were used.

## Workspace activity popover smoke setup correction — 2026-11-01

- Packaged workflow [41344894097](https://github.com/ashutoshpw/zmux/actions/runs/41344894097), source SHA `71759b3a8294deaa0d65e7f7e1c6542fafdf689c`, failed only `popover_shown`. The recorded details, status/log fields, plain-text/read-only settings, and vertical scroller all passed. Diagnostics: `/tmp/zmux-41344894097-diagnostics/native-smoke.json.stages.json` and `native-stderr.log`.
- Source review found the smoke had hidden `sidebarSections` through `showNotifications()` and only swapped the scroll document when restoring Spaces. That left the workspace row under a hidden ancestor: the activity controller and text existed, but AppKit could not show a popover anchored to that hidden row.
- The metadata smoke now uses `showWorkspaces()` and verifies responder identity is unchanged, scrolls the target activity button into the Spaces clip view, and verifies the whole button is visible before its synthetic click. The `popover_shown` and all content/focus acceptance checks remain intact.
- `git diff --check -- apps/macos/Sources/App.swift` passed. This Linux host has no `swiftc` or `swift-format`; packaged AppKit confirmation remains pending.

## Workspace activity smoke visibility setup verification — 2026-11-01

- The full `RUST_TEST_THREADS=2 scripts/check` passed locally on Linux with exit status 0 (r35). Log: `/tmp/zmux-agent-sidebar-check-r35.log`.
- Hosted workflow [41344894097](https://github.com/ashutoshpw/zmux/actions/runs/41344894097) passed app/core/selection checks, then failed only `popover_shown` in native smoke; all diagnostic content checks passed. The source smoke had left the Spaces section under a hidden ancestor. It now restores the real workspace view and ensures the button is visible before clicking. Independent review covered the smoke setup and preserved assertions.
- A hosted macOS rerun is pending. The local Linux check does not establish packaged AppKit acceptance.

## Workspace drag destination visibility smoke correction — 2026-11-01

- Packaged workflow [41348156262](https://github.com/ashutoshpw/zmux/actions/runs/41348156262), source SHA `03ca1a1de30bee81f24056703f0f07cf8181aaf1`, passed the prior workspace activity popover smoke and then failed `WorkspaceOrderSmoke` because its after-group destination was outside the visible sidebar viewport. Diagnostics: `/tmp/zmux-41348156262-diagnostics/native-smoke.json.stages.json` and `zmux-workspace-drag.png`.
- The smoke intentionally shrinks the window to exercise the independently scrolling Spaces list. Moving the fixture ahead of its group leaves the next insertion point below the current clip view; the assertion previously stopped before scrolling to that destination.
- The drag smoke now starts the source row first, reveals the destination through the actual sidebar document view's `scrollToVisible` and enclosing `NSScrollView`, verifies the drop point is visible and selection/focus/content remain unchanged, then runs the same real `WorkspaceSidebarList` insertion and drop callbacks. No ordering or callback acceptance check was removed.
- Production drag destination code was not changed. The hosted fixture uses direct AppKit callbacks rather than an OS drag session, so this change does not prove native edge auto-scroll behavior. `git diff --check` passed; this Linux host has no `swiftc` or `swift-format`. Hosted macOS rerun remains pending.

## Stalled SSH pressure snapshot fallback — 2026-11-01

Foundation retry r36 failed because `Lost` arrived before a Heartbeat had observed the full 400 ms pressure interval. The joined producer snapshot showed 10,187 accepted and 3,057,414 rejected receipts, while the latest valid Heartbeat (sequence 19) had the same accepted count and 2,742,959 rejections; the sustained Heartbeat marker was therefore still unset even though the producer-side rejection epoch had started earlier.

The test now retains the producer rejection epoch observed by the latest valid Heartbeat and evaluates pressure against the stopped, joined pump snapshot on `Lost`. It requires at least one valid Heartbeat, unchanged accepted count, strictly increased rejected count, an unchanged rejection epoch, and at least 400 ms elapsed from that producer epoch. The pressure marker is the rejection epoch start plus 400 ms. This keeps the evidence tied to a valid sequenced peer frame and a stable producer snapshot without requiring a later Heartbeat to arrive before loss.

The 30-second absolute loss deadline, 2.45-second pressure-relative loss bound, strict binding and sequence checks, and cancellation/disconnect phase remain in place. No sleeps or production changes were added.

Focused Linux verification:

- `RUST_TEST_THREADS=2 cargo test -p zmux-desktop-runtime remote_agent_lifecycle::tests::real_managed_ssh_stalled_stdin_has_bounded_loss_and_cancellation -- --nocapture` passed. The pressure marker was 653 ms after production began; `Lost` followed 1.610 seconds later, within the 2.45-second bound. Cancellation disconnected the sender 110 ms after stream drop.
- `RUST_TEST_THREADS=2 cargo test -p zmux-desktop-runtime --lib` passed all 115 tests. Log: `/tmp/zmux-stalled-snapshot-fallback-r1.log`.
- `RUST_TEST_THREADS=2 scripts/check` passed with exit status 0. Log: `/tmp/zmux-stalled-snapshot-fallback-r2.log`.
- `cargo fmt --all -- --check` and `git diff --check` passed.

These are local Linux source checks; no hosted rerun or commit was started for this test-only follow-up.

## Agent selection PTY inventory assertion correction — 2026-11-01

- Hosted workflow [41352950592](https://github.com/ashutoshpw/zmux/actions/runs/41352950592), source SHA `84fe780c42df9c0add8fe1412232b74c92258ed2`, passed native rendering and workspace drag, then failed the agent-selection no-replacement assertion. The exact target workspace/pane/surface/session selection checks had already passed. Screenshot: `/tmp/zmux-41352950592-diagnostics/zmux-sidebar-failure.png`.
- Source review confirmed `DesktopSnapshot.frames` is filtered to workspaces selected by their owning windows, while `Snapshot.control_frames` contains all live terminal sessions. Comparing the visible frame dictionary across a focus change was therefore not a complete PTY inventory.
- The smoke now reads `list-pane-surfaces --pane` for every pane before and after selection. These production targeted reads use `control_frames`; the smoke compares exact owner, surface UUID, kind, session ID and process ID across every surface, and requires the target PTY to remain running with its original PID. The no-extra/no-replacement checks remain strict.
- `git diff --check -- apps/macos/Sources/App.swift` passed. This Linux host has no Swift/AppKit compiler; hosted macOS verification is pending. No commit was made.

## Agent selection inventory review and local verification — 2026-11-01

- Static review found that the acceptance receipt still referenced the removed `noReplacement` variable. It now derives from the complete before/after surface inventory comparison and the target PTY PID equality, and the same value gates the smoke and appears in its receipt.
- Each targeted pane response now validates row-level window/workspace/pane ownership, rejects duplicate surface IDs, records the owner window UUID, and binds the target window/surface UUIDs to the lifecycle record.
- `list-pane-surfaces` and `DesktopSnapshot` expose the same global snapshot revision, which can advance for unrelated runtime changes; the per-pane reads are not atomic. The smoke therefore checks each response against that pane in its associated model and compares full stable identities before/after selection, without requiring unrelated global revisions to match.
- `RUST_TEST_THREADS=2 scripts/check` passed locally on Linux with exit status 0 after the inventory update. `git diff --check -- apps/macos/Sources/App.swift` passed. This host has no Swift/AppKit toolchain; hosted macOS typecheck and packaged acceptance remain pending.

## Hosted Swift warning correction — 2026-11-01

- Workflow [41356715298](https://github.com/ashutoshpw/zmux/actions/runs/41356715298) stopped at native Swift typechecking because the target guard bound `processID` without using the bound value. The guard now checks `processID != nil`, preserving the requirement that the selected target has a process ID while keeping the optional value available for inventory comparison.
- Reviewed the rest of the new inventory block: its bindings are consumed by exact owner, UUID, model, duplicate, completeness, identity, or diagnostic checks.
- The full `RUST_TEST_THREADS=2 scripts/check` r38 passed locally on Linux after the `processID != nil` correction; log: `/tmp/zmux-agent-sidebar-check-r38.log`. `git diff --check -- apps/macos/Sources/App.swift` passed.
- Workflow 41356715298 stopped at Swift warnings-as-errors before building or running the native app. This Linux host cannot run the native typecheck; a hosted rerun remains pending. No commit or workflow rerun was started for this correction.

## Darwin accepted-socket mode in provider-hook fixture — 2026-11-01

Hosted workflow [41357406209](https://github.com/ashutoshpw/zmux/actions/runs/41357406209) failed `apps/cli/tests/provider_hooks.rs::local_hook_ipc_sends_only_minimal_lifecycle_fields` at `read_event_frame` with `WouldBlock`. The fixture's `UnixListener` was nonblocking, while the accepted stream went directly to `read_exact`; on Darwin, accepted Unix sockets can inherit the listener's nonblocking mode.

The provider-hook fixture now switches each accepted stream to blocking mode and sets a three-second read timeout before reading the bounded frame. All existing privacy assertions remain unchanged, including omission of prompt/tool/private unknown fields and the expected redacted agent ID. Audit found the same missing mode switch in the negative local-supervisor fixture; that accepted stream now switches to blocking mode before its existing two-second bounded frame read. Neither change touches production code or authorization behavior.

Unix-stream accept audit: the other nonblocking-listener test readers already set accepted streams blocking with read timeouts (`execution_admission`, `browser`, `windows`, and `open`), or deliberately retain nonblocking mode with deadline-based `WouldBlock` loops (`local_zellij_owner/observer`). Production control and supervisor listeners configure accepted sockets explicitly; Claude hook serving, scheduler, broker, completion-control, and local-zellij worker paths use explicit nonblocking I/O or readiness/deadline loops.

Focused Linux verification:

- `RUST_TEST_THREADS=2 cargo test -p zmux --test provider_hooks local_hook_ipc_sends_only_minimal_lifecycle_fields -- --nocapture` passed.
- `RUST_TEST_THREADS=2 cargo test -p zmux --test provider_hooks -- --nocapture` passed: 10 passed, 1 ignored.
- `RUST_TEST_THREADS=2 cargo test -p zmux --test local_zellij -- --nocapture` passed: 3 passed.
- Full `RUST_TEST_THREADS=2 scripts/check` r39 passed locally on Linux; log: `/tmp/zmux-agent-sidebar-check-r39.log`.
- `cargo fmt --all -- --check` and `git diff --check` passed. A hosted Darwin rerun remains pending; no commit was made.

## Hosted universal build and packaged sidebar acceptance — 2026-11-01

This verifies the delivered native build and the implemented agent-selection smoke slice. The broader live-agent sidebar task remains in progress; unsupported providers, alert-delivery limitations, and physical/real-provider evidence gaps below remain open.

- macOS development workflow [41358822811](https://github.com/ashutoshpw/zmux/actions/runs/41358822811) passed for source SHA `d348aab6dff4644c36f643a53acac0e4f9d7032e`. Download: [universal development build artifact 13105106870](https://github.com/ashutoshpw/zmux/actions/runs/41358822811/artifacts/13105106870). Inner ZIP SHA-256: `36ac1815a4768c27d43b50e83d4659c76d6d092242fc7fc20f26ad9691d1d75f`.
- Build target: universal macOS 14+, arm64 and x86_64. Signing: ad hoc. Notarization: none.
- Foundation workflow [41358822843](https://github.com/ashutoshpw/zmux/actions/runs/41358822843) passed on both macOS and Ubuntu for the exact source.
- Packaged sidebar smoke, status menu checks, and agent selection passed. Selection verified the full window/workspace/pane/surface/session identity and unchanged surface/PTY inventory. The packaged screenshot was inspected; it uses a synthetic fixture and does not establish live provider behavior.

Remaining limitations and acceptance gaps:

- Codex lifecycle support is not implemented. The automatic local Claude adapter was verified with Claude Code `2.1.301`; other versions remain unverified. The remote adapter requires Python 3 on the remote host.
- A lost alert receipt can leave the visual indication without sound. Alert delivery is at-least-once, so duplicate delivery remains possible.
- Physical Mac use, audible real-provider events, and authenticated remote-provider behavior remain unverified.
- The broader task is not complete; its unchecked design, provider, sound-preference, and remote-acceptance criteria remain open.

## Provider compatibility follow-up — 2026-11-02

- User reports Claude Code 2.1.316 is rejected and requests Codex/OpenCode support in the same Agents module. Scope includes lifecycle projection, exact terminal ownership and attention/completion alerts for local and managed SSH sessions.
- Inspection confirmed the local Claude gate accepts only exact 2.1.301, and the CLI rejects Codex explicitly. Preserve current near-black UI, user configuration and provider trust; do not use hook-trust bypasses or substitute an unrelated provider session.
- Plan: verify provider contracts; implement bounded provider-specific adapters using existing lifecycle/transport owners; add behavioral regressions at those owners; obtain independent Luna review; run scripts/check; deliver and verify a fresh universal macOS artifact.
- Existing unrelated cloud-origin task edits, profile-lifecycle workflow and native-profile-lifecycle tool are outside this follow-up.

### Accepted provider discovery

- Claude: exact 2.1.301 gate is the observed compatibility defect; installed 2.1.316 retains repeatable plugin loading. Implement capability-checked compatibility and accurate version metadata, then CLI-boundary regression checks.
- Codex 0.166.0: hooks run concurrently with user hooks; request/Stop observations are not post-handler outcomes. Preserve normal /hooks trust and approval decisions. Further focused research is checking whether the existing CLI completion notify/event channel can be composed without overwriting user settings.
- OpenCode v2.2.1: invocation-scoped plugin receives session.status busy/retry/idle plus question/permission events. Bind root session exactly; never conflate subordinate/global sessions. Experimental idle alone is insufficient for completion unless paired with idle and no background tasks.
- Shared design needs provider variants plus bounded provider-ID-to-UUID namespacing, preserving Zmux runtime/generation UUIDs, owner capabilities, SSH nonce/sequence fencing and Unknown on evidence loss.
- Research sources: https://code.claude.com/docs/en/hooks ; https://code.claude.com/docs/en/changelog#2-1-316 ; https://learn.chatgpt.com/docs/hooks ; https://opencode.ai/docs/plugins/ ; https://opencode.ai/docs/config/ . No authenticated provider turn was executed during discovery.

### Shared provider implementation started

- Accepted Codex follow-up: 0.166.0 existing shared daemon documents thread/read/list/turns/list, thread/status/changed and turn/started/completed. Active wait flags distinguish approval/input from Working. Completion must identify a newly observed completed turn. Never call thread/start to monitor another CLI.
- Exact Codex-to-terminal launch association remains mandatory: the daemon record has no PTY/PID/surface. Cwd, title, or a single cwd candidate are not sufficient identity evidence. Normal trusted SessionStart integration may supply the exact thread binding; live daemon proxy attachment was not established by the read-only probe. Preserve notify because invocation overrides replace it.
- Shared implementation is assigned independently of the concurrent Claude compatibility edits: provider variants, opaque ID mapping, silent baseline/recovery semantics, managed SSH frames/owner routing and native provider labels. Later adapter producers will consume its documented interface.
- Sources: https://learn.chatgpt.com/docs/app-server#api-overview ; https://learn.chatgpt.com/docs/config-file/config-advanced#notifications . Installed-schema inspection is contract evidence, not live callback acceptance.

### Claude compatibility slice accepted — local evidence

- Local gate now accepts 2.1.301–2.1.316 inclusive with repeatable --plugin-dir capability validation, bounded help capture, executable identity revalidation and actual version persistence. Existing valid 2.1.301 records remain accepted/idempotent. Future patches remain outside the reviewed band.
- Worker reproduced the 2.1.316 CLI regression before the fix and passed the focused suite afterward against isolated baseline source with scoped changes: 11 passed, 1 ignored. cargo build --locked -p zmux and package formatting passed there. Installed 2.1.316 temporary install/status/uninstall also passed without a model/API turn or global config edit.
- Shared checkout integration was still compiling the generic provider slice; source-wide checks, independent review, hosted/native acceptance and a new artifact remain pending.
- Producer implementation is assigned for local/managed SSH Codex and OpenCode, consuming the shared normalized contract. Codex attachment must bind exact root thread using a supported launch association; no cwd-only guessing, trust bypass or notify replacement.

### Shared lifecycle slice accepted — focused source checks

- Generic Codex/OpenCode local/remote variants, opaque session/turn UUIDv5 projection and native provider labels implemented. Public local lifecycle actions admit observations and explicitly invalidate on evidence loss/provider exit; local control cannot claim a managed SSH route.
- Managed SSH protocol v3 adds provider_state observations; every matching provider record follows transport heartbeat/loss/reconnect. Baseline/recovery, failures, interruptions and loss stay silent; completion requires the exact turn's prior live Working observation.
- Worker checks passed: remote protocol tests (2); desktop lifecycle unit tests (14); desktop lifecycle integration tests (3); focused managed SSH gap and multi-provider reconnect; strict Clippy for both runtime crates; cargo fmt --all -- --check; git diff --check. These are scoped source checks, not full scripts/check, native package or real-provider acceptance.
- Independent Luna review of the completed shared/Claude slices has started. The launch/plugin/daemon producer integration remains in progress in disjoint files.

### Independent review: provider recovery corrections required

- Luna review found three deterministic P2 regressions in generic recovery: a different-turn delayed baseline clears a live turn's completion eligibility; a silent Waiting snapshot does not suppress a replayed wait; replacing a locally lost record discards its turn dedupe histories.
- Accepted fixes and regression interleavings are assigned to a dedicated runtime owner, disjoint from the ongoing CLI/shell producer implementation. Recovery must retain bounded observed-turn histories and generation fences while clearing completion eligibility, and never produce historical sounds.
- Review found no additional issues in the completed Claude capability/version slice, provider labels, local-versus-SSH owner admission, capability-free receipts or multi-provider remote transport handling. Review did not establish live provider behavior. Full checks and packaging remain pending.

### Recovery review fixes accepted — focused checks

- Fixed all three review traces through production control-path regressions: same-generation silent snapshots preserve accepted live-turn state; silent Waiting snapshots mark the wait observed; local loss recovery reuses the provider-session record with dedupe/outcome history intact. A fourth trace fences delayed retired runtime/generation claims across repeated local loss.
- Histories and retired identities each retain 128 marks per provider session; older marks can be evicted. Recovery clears completion eligibility, so no baseline can synthesize a completed run.
- Each interleaving failed before the correction and passes afterward. Lifecycle unit tests (14), integration tests (3), desktop-runtime Clippy, scoped formatting and git diff --check passed.
- Adapter producer implementation remains in progress; final independent integration review, full scripts/check and required universal artifact are not complete.

### Producer slice implemented — integration acceptance in progress

- Local and managed SSH Codex/OpenCode wrappers implemented. Codex preserves effective SessionStart hook entries and normal /hooks trust, obtains exact thread identity and reads the existing daemon; no notify override, approval decision or Stop-as-completion. OpenCode uses a standalone invocation-scoped plugin, preserving custom/global/project configuration and existing plugins.
- Both providers use bounded sanitized events and true five-second source snapshots; remote frame ordering shares the managed lifecycle sequence and explicit loss/gap invalidates evidence.
- Worker focused checks passed: CLI provider hooks 13 passed/2 ignored; SSH 3; CLI unit 12 passed/1 ignored; session lifecycle 16; shell integration 10 passed/2 ignored; strict Clippy, Rust formatting, Python/Node syntax and diff checks.
- Installed OpenCode v2.2.1 accepted the plugin and empty-root callback without provider credentials/model turn. Installed Codex 0.166.0 accepted read-only daemon initialization/listing, but no thread was loaded: authenticated turn notifications remain unverified.
- Independent integration/recovery review, full RUST_TEST_THREADS=2 scripts/check and expanded packaged native multi-provider acceptance are running. No scoped change has been committed or pushed yet, and no new accepted artifact exists.

### Linux validation sub-agent — 2026-11-02

- `RUST_TEST_THREADS=2 scripts/check` completed with exit 101 and stopped in `cargo test --workspace --locked`. The first failing target was `-p zmux --test agent_messages`: `session_local_hook_adapter_uses_actual_terminal_owner_and_never_writes_message_input` failed during synthetic plugin installation because its inline fake Claude handles `--version` but not the now-required `--help` capability probe. The CLI reported `Claude --help preflight did not exit successfully within its bounds`. Log: `/tmp/zmux-provider-support-check.log`.
- To finish the workspace test binaries after Cargo's default fail-fast behavior, `RUST_TEST_THREADS=2 cargo test --workspace --locked --no-fail-fast` completed with exit 101. Three targets failed: the same `agent_messages` fixture; `-p zmux-desktop-runtime --lib`, where `remote_agent_lifecycle::tests::retries_a_real_managed_ssh_bootstrap_race_then_reports_session_loss` logged two closed streams (`hello=false, active=false`) and panicked `remote lifecycle stream was lost before hook` at `crates/desktop-runtime/src/remote_agent_lifecycle.rs:1211`; and `-p zmux-remote-runtime --test connection`, where `managed_ssh_shell_routes_nonce_bound_opener_requests_from_loopback_sshd` and `configured_remote_hooks_fallback_for_unreadable_payloads_and_ignore_other_notifications` received `gap` instead of `hook`, while `required_remote_receipt_failures_emit_a_generation_gap` received exit code 73 instead of 74. Log: `/tmp/zmux-provider-support-tests.log`.
- The remaining `scripts/check` stages were run separately because its fail-fast test stage prevented them from running: `cargo clippy --workspace --all-targets --locked -- -D warnings` passed; `scripts/license-inventory --check` passed with 134 resolved packages, 100 distinct notice texts, and 3 packages without bundled top-level license text; `scripts/check-native-bridge` passed both C ABI checks. Logs: `/tmp/zmux-provider-support-clippy.log` and `/tmp/zmux-provider-support-native-bridge.log`.
- The 3 packages lacking bundled license text (`r-efi`, `winapi-i686-pc-windows-gnu`, and `winapi-x86_64-pc-windows-gnu`) match the baseline inventory. The new direct `tungstenite 0.20.1` dependency is configured with default features disabled and only `handshake`; its notice inventory is current.
- Installed OpenCode v2.2.1 and Codex 0.166.0 read-only acceptance was not repeated because the preceding worker evidence in this status covers those checks; Codex had no loaded thread. No credentials, provider turn, user configuration, or daemon state was changed by this validation.
- The host is Linux. AppKit compilation and packaged universal macOS acceptance are not established by these checks. No production or test files were changed, and no commit, push, or hosted workflow was run.

### Full-check remediation — 2026-11-02

- Updated the existing inbox C provider fixture to advertise repeatable --plugin-dir on --help. The focused real-terminal inbox test now passes (all held/plan/retirement scenarios); no production behavior or test assertion was weakened. Log: /tmp/zmux-provider-inbox-fixture-fix.log.
- Restored the existing v3 Claude hook envelope without a generation field in the managed stream; new generic provider observations still require explicit current generation, and any supplied stale generation is ignored. Unconditional generation enforcement had gapped existing Claude hook traffic before receipt handling.
- Existing managed SSH connection suite now passes all 9 tests, including required-receipt failure semantics, unreadable payload fallback and loopback opener/hook routing. The managed SSH bootstrap-race/session-loss regression passes. Logs: /tmp/zmux-provider-remote-envelope-fix.log and /tmp/zmux-provider-bootstrap-fix.log.
- Regenerated dependency notices using scripts/license-inventory for tungstenite and transitive packages. The three preexisting missing top-level license-text entries are unchanged.
- RUST_TEST_THREADS=2 scripts/check rerun started at /tmp/zmux-provider-support-check-r2.log; independent provider review and native fixture extension remain running.

### Packaged multi-provider fixture implemented — native run pending

- Native sidebar fixture now seeds Claude Working, Codex Waiting and OpenCode Idle against the exact existing terminal owner through real production lifecycle control. Generic providers use opaque IDs and silent baseline observations. Inputs are explicitly synthetic, not real provider execution or audible acceptance.
- Smoke validates all three labels/groups, reveals rows in the independent agent scroll view, selects each from another workspace and verifies the complete unchanged surface/session/PTY inventory per selection. The populated screenshot is captured with all groups visible.
- Python syntax/help, Swift source-input glob closure and diff checks passed locally; this Linux host has no Swift/AppKit compiler. Independent fixture review and universal hosted compilation/execution remain pending.

### Full portable validation passed

- RUST_TEST_THREADS=2 scripts/check completed with exit 0 after the fixture and Claude v3 envelope fixes. Log: /tmp/zmux-provider-support-check-r2.log. This includes workspace tests, strict workspace all-target Clippy, dependency license inventory and native bridge ABI checks.
- git diff --check passed. Native AppKit/Swift compilation and packaged multi-provider smoke remain hosted gates; the passing Linux suite does not establish a native build or authenticated provider-turn behavior.
- Independent integration and packaged-fixture reviews remain pending. No source commit/push or new build workflow has started.

### Native fixture review corrections

- Fixed row lookup to match the provider label independently of subtitle separator position; default Terminal titles place the provider at the end of the subtitle.
- Selection now checks the latest lifecycle activity/connection and current accessibility status immediately before the click, after focus, and after full PTY inventory collection. Acceptance identities are built from fresh final lifecycle records, never the initial captured records. Expired evidence fails the smoke instead of reporting initial Working/Waiting/Idle values.
- Independent review confirmed actual control/request shapes, identity-keyed native row reuse and unchanged complete inventory assertions. Python syntax and git diff --check pass; AppKit warnings-as-errors and packaged execution remain hosted gates.

### Final integration review fixes implemented

- Codex invocations selecting a directory/profile now pass through untouched without tracking, because config/read cannot reproduce an explicitly selected profile/context. Local mcp passthrough now matches the remote adapter. Actual terminal launch regressions verify unchanged arguments, preserved hooks and a single provider launch for these cases.
- Codex root records accept omitted/null parentThreadId and reject non-null parent IDs. The shared daemon fixture now omits the optional field, so both actual local and loopback-SSH adapter paths cover that valid schema case.
- A normally ended persisted local session can resume with a fresh runtime/generation, retaining histories and retired-identity fences. The production control regression checks resumed Working/Connected and rejects the old generation.
- Accepted local event sockets use blocking mode plus an absolute 200 ms frame budget. A real trickling peer can no longer extend every byte's deadline and hold teardown for minutes.
- New remote provider FIFO writes obey the actual atomic limit: Python fpathconf, Node bounded/cached getconf with POSIX-minimum fallback. Oversized atomic frames report loss before writing; protocol frame bounds remain 4096 bytes.
- Before-fix regressions reproduced omitted-root rejection, normal-exit resume rejection, changed cd/profile/mcp invocation args, unbounded trickle reception and oversized FIFO emission. Relevant logs: /tmp/zmux-provider-root-before.log, /tmp/zmux-provider-resume-before.log, /tmp/zmux-provider-trickle-before.log, /tmp/zmux-provider-context-before.log, /tmp/zmux-provider-context-v2-before.log, /tmp/zmux-provider-pipebuf-before-r2.log.
- After-fix focused checks passed: CLI unit tests 13/1 ignored, provider hooks 13/2 ignored, SSH 3, lifecycle integration 3 and shell/plugin integration 11/2 ignored. Logs: /tmp/zmux-provider-review-cli-after-r2.log, /tmp/zmux-provider-review-runtime-after.log, /tmp/zmux-provider-review-plugin-after-r2.log. Final full-suite rerun and targeted independent re-review follow.

### Final recheck and full-suite rerun

- All six integration findings are fixed with the scope and limitations recorded in provider-adapters.md. A targeted independent Luna recheck also includes the native fixture label/freshness corrections.
- Full RUST_TEST_THREADS=2 scripts/check r3 is running at /tmp/zmux-provider-support-check-r3.log. Socket connects now use the existing bounded socket2 pattern as well as absolute incoming-frame deadlines.
- No new authenticated provider turn, Mac build, physical audio acceptance, source commit or push is claimed at this stage. The next delivery gate is clean review plus complete source checks, followed by cohesive commits and the existing universal workflow.

### Full portable revalidation passed after final review fixes

- RUST_TEST_THREADS=2 scripts/check r3 completed with exit 0. Log: /tmp/zmux-provider-support-check-r3.log. Workspace tests, strict all-target Clippy, license notices and both native bridge ABI checks passed.
- The required macOS development workflow and Foundation checks are active in ashutoshpw/zmux. Native compilation/packaging remains unexecuted for these changes; final independent recheck is pending before cohesive commits and source-bound universal build delivery.

### Final recheck accepted; passthrough proof completed

- Independent recheck found no remaining source defects in the six corrections or native label/freshness fixes. It identified incomplete remote launch-count coverage. The managed SSH fixture now exports its launch counter and asserts exactly one provider launch per directory/profile/mcp passthrough. Both local and remote matrices also cover -C and -p short options.
- Full RUST_TEST_THREADS=2 scripts/check r4 is running at /tmp/zmux-provider-support-check-r4.log for the final test-only additions. Core provider admission/selection files are staged as the first cohesive slice; unrelated work remains unstaged. No commit/push/build yet.
- Optional non-null Codex parent rejection remains source-checked; omitted root parent and foreign-thread isolation are exercised through actual local/SSH fixtures. No live provider turn is claimed.

### Source delivery commits — final portable checks green

- RUST_TEST_THREADS=2 scripts/check r4 completed with exit 0 for the final local/SSH passthrough counter and short-option coverage. Log: /tmp/zmux-provider-support-check-r4.log.
- Core lifecycle admission, recovery/dedupe and provider labels committed as 1a589a5. Local/managed SSH provider adapters, Claude compatibility, regression coverage, usage docs and generated dependency notices committed as 50ecf71. All commits use explicit scoped paths; unrelated cloud-origin/profile-lifecycle work remains untouched.
- The native multi-provider smoke and task evidence form the final source slice. Swift/AppKit compilation, actual packaged rendering, universal architectures/signature verification and final artifact download are the remaining delivery gates.
