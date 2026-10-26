# Cross-cutting families and source-only additions

Source comparison: cmux `3ba653b9e5afb11d0bc578031142950d6136697e`, zmux
`69342a8f036037736c6eb9a500e479ac41e32d93`, 2026-10-15. Added during independent
coverage review. No runtime, permission or managed-profile changes were performed.

## Computer Use — missing

This is separate from browser automation and ordinary agent terminal launch.
`docs/computer-use.md:1`, `Sources/AppDelegate.swift:1008` and
`Sources/App/ComputerUseUXCoordinator.swift:8` describe/own the authenticated helper,
per-launch agent injection, live-session projection, menu-bar state and onboarding.
`Packages/macOS/CmuxComputerUse/Sources/CmuxComputerUse/ComputerUseRuntimeService.swift`
owns the helper runtime; its onboarding and capture extensions own readiness checks.

The reference bundles a distinct helper with its own signing/TCC identity. Supported
Claude and Codex launches receive an authenticated, variant-scoped MCP endpoint; Codex
uses its own ten-tool compatibility profile and approval protocol, while Claude has a
broader tool surface. This does not mean replacing Codex's bundled Computer plugin.
Desktop screenshots/accessibility trees, click/type/selection/scroll/drag/key/app/window
operations have profile-specific targets and errors. Snapshot element references must
not survive an unrelated snapshot. Approvals belong to the authenticated client harness.

First functional use can claim one onboarding presentation when setup is needed.
Passive discovery, launch, readiness refresh and helper recovery must not repeatedly
raise permission UI. Readiness depends on helper identity, actual grants and verified
capture, not dismissing an onboarding window. Disable, revoked permissions and helper
replacement invalidate readiness. Optional global skill installation only manages
verified app-owned links; user/project skills remain intact. Settings include enabled
and menu-bar visibility, with policy overrides. Reference telemetry/self-update behavior
is controlled by the host.

zmux has no helper target/runtime, MCP injection, Computer Use settings or approval/TCC
owner (`apps/macos/Sources/App.swift`, `CloudSettings.swift`, `apps/cli/src/main.rs`).
Ordinary terminal keyboard input and WebKit navigation are not substitutes.

**Closure:** independently implement and license-review any new helper dependency;
use zmux-owned signed identities, isolated client credentials and shared lifecycle state.
Verify supported client/version profiles, rejected foreign/stale clients, exact tool
schemas, foreground/background focus behavior and unsupported actions. Exercise real
Accessibility/Screen Recording denial/grant/revocation, onboarding claim concurrency,
helper crash/replacement, stale completion rejection, session end and menu-bar cleanup.
No browser page or generic remote relay may acquire desktop-control authority.
Dependencies: agent launch identity, signed distribution, policy owner and native macOS.
Phase P6/P8; priority P0 security design, later implementation. No source was imported.

## Managed-device policy — missing

The policy boundary is broader than user configuration. See
`docs/managed-device-policies.md:27`,
`Packages/macOS/CmuxSettings/Sources/CmuxSettings/Policies/ManagedDevicePolicy.swift:24`,
`ManagedDevicePolicyKey.swift` in the same directory,
`Sources/AppDelegate+ManagedPolicy.swift:12` and
`Sources/ManagedPolicyEnforcementObserver.swift`.

Reference policies cover embedded browser and URL/local-file/loopback access, remote
control/discovery/incoming access, cloud, SSH/mosh, transfers, Iroh, Computer Use,
custom sidebars, AI credential upload, automation webhooks, TLS trust bypass,
telemetry, updates and restrictive socket modes. Forced preferences take precedence
over user settings/config/env; ordinary defaults writes cannot impersonate MDM.
Disable-style booleans, allow-style booleans and restrictive enums have different
validation/default semantics. Invalid forced socket modes fail closed. Channel builds
consult the release policy domain when appropriate.

Enforcement is shared across UI, CLI, automation, restored state and live resources.
Changes can close browser surfaces, revoke socket clients/event subscriptions, stop
helpers and disconnect owned remote sessions. Reconnect must not circumvent a new
restriction. Policy removal restores the effective user choice. Telemetry/update have
launch-time effects; other owners reconcile on notifications/activation and bounded
periodic refresh. Locked controls and policy errors must explain the effective state.

zmux currently enforces same-user socket ownership and URL safety, but has no managed
policy resolver, forced-preference detection, policy generation or transition observer.

**Closure:** define zmux policy keys/domain and one typed effective-policy snapshot.
Test forced versus ordinary values, malformed types, cross-channel resolution and
composition. Verify every affected entrypoint and live disable/re-enable transition,
including client revocation and cancelled retries/uploads. Run controlled profile tests
on a dedicated Mac and prove user config/env cannot override policy. Do not install
profiles on the user's working machine as part of this audit. Phase P0/P4/P8, priority P0.

## Project pane — missing

`CLI/cmux.swift:10031` accepts `project open <path>` or the single-path shorthand,
`--workspace`, `--window` and `--focus`. Paths expand relative to the CLI cwd;
caller workspace defaults apply without an explicit window; focus defaults true.
It calls `project.open` and formats returned window/workspace/pane/surface IDs and path.

The owning coordinator is
`Packages/macOS/CmuxControlSocket/Sources/CmuxControlSocket/Coordinator/Project/ControlCommandCoordinator+Project.swift:45`.
It validates route/path existence, distinguishes unavailable manager, missing workspace,
missing focused pane and creation failure, and returns created identities.
`Sources/Panels/ProjectPanel.swift:50` owns asynchronous project loading and files,
targets, build settings and schemes tabs; selected file/target/scheme/configuration,
filters and reload failures are separate state. The coordinator also has `project.set_*`
and `project.get_state` methods; their debug availability must follow the actual build.
This is not just opening an external editor or displaying a file tree.

zmux has only terminal/browser surface kinds and no project model, parser, pane or
command. **Closure:** original project/workspace parser and non-PTY surface; shared
CLI/UI path routing, safe asynchronous loading, explicit parse/missing-file states,
stable target/focus behavior and correct result IDs. Test `.xcodeproj`/`.xcworkspace`,
spaces/relative paths, nonexistent/malformed projects, reload and close-during-load,
selection/settings views and build-specific debug capability exposure. Phase P5, priority P1.
`openFolder` (UX-ACTION-019) is separately routed to CU-14's general open/content contract.

## Quit and application lifecycle — partial

`Sources/AppDelegate.swift:2461` handles pending quit requests, user/update reasons,
configurable dirty-workspace confirmation, single confirmation ownership, inspector
teardown, deferred owned cleanup and a fresh session snapshot. Its termination watchdog
and agent cleanup prevent indefinite shutdown or duplicated teardown.

zmux `apps/macos/Sources/App.swift:1170` exits after the last window and uses deferred
asynchronous controller shutdown, with a guard against repeat termination and visible
stderr on cleanup failure. This is an implemented subset, not equivalent quit policy:
there is no configurable dirty-workspace confirmation, persisted session snapshot,
update-relaunch or rich remote/agent cleanup lifecycle.

**Closure:** unify Cmd-Q/menu/last-window/system/update termination reasons; preserve
cancelled quit, avoid duplicate dialogs, bound owned cleanup, persist recoverable state
before exit, and prove cancelled/background operations cannot revive closed objects.
Test live local/remote jobs, dirty content, browser download/inspector and update restart
without killing unrelated processes. Phase P1/P4/P8; priority P0 lifecycle. This explicitly
covers UX-ACTION-009; routing an identifier alone is not evidence of handler coverage.
