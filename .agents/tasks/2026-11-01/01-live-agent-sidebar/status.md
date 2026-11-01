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
