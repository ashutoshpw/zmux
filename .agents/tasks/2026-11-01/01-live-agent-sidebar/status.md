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
