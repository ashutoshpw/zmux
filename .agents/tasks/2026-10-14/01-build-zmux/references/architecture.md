# Proposed architecture

## Product boundary
Zmux is a native terminal workspace app with a command-line automation interface.
The first release provides local shells, tabs, pane splits, workspace switching,
layout restoration, and explicit notifications for agent workflows. Remote execution
is a separate capability. Cloud accounts, billing, browser embedding, collaboration,
and iOS are outside the initial local MVP.

## Monorepo boundaries and ownership
Paths below are proposals, not existing packages. Each row can become an independently
owned workstream after its upstream contract is defined. Start with one repository;
publish separate artifacts only when a concrete consumer needs them.

| Proposed path | Responsibility | Dependencies / proposed technology |
| --- | --- | --- |
| `packages/workspace-core/` | Stable IDs, split tree, actions, state transitions, effects | Pure Swift; no AppKit or networking |
| `packages/terminal-adapter/` | Engine abstraction, surface lifecycle, PTY events | Swift/C boundary to a reviewed upstream engine |
| `packages/workspace-storage/` | Versioned snapshots, atomic saves, migrations | Swift; core serialization contracts |
| `packages/automation-protocol/` | Schemas, versioning rules, error codes, original fixtures | Language-neutral JSON schemas |
| `packages/local-control/` | Socket lifecycle, peer validation, action dispatch | Swift; protocol and core |
| `apps/macos/` | Native windows, panes, accessibility, settings | AppKit shell; SwiftUI where suitable |
| `apps/cli/` | CLI parsing and API client | Swift initially; no independent business rules |
| `services/remote-agent/` | Remote PTY ownership, stream replay, session cleanup | Rust; Linux first |
| `packages/remote-transport/` | SSH connection lifecycle and stream framing | Swift client; documented wire protocol |
| `services/control-plane/` | Device registry, identity, presence, optional relay grants | TypeScript; PostgreSQL proposed |
| `services/relay/` | Authenticated forwarding with quotas | Choose runtime after transport feasibility |
| `apps/web/` | Optional device/account UI | TypeScript; control-plane API |
| `apps/ios/` | Remote session presentation and input | Swift; remote client contracts |

## Shared action flow
UI, shortcuts, and local automation submit typed actions to a workspace coordinator.
The core validates actions and computes state changes plus explicit effects. Effect
handlers own PTYs, persistence, notifications, and networking. Asynchronous completion
returns typed events to the coordinator. Serial state mutation prevents races between
UI and automation; effects must not block that executor.

Example actions: create workspace, split pane, focus pane, close session. Actions
carry stable object IDs and request context. Session closure is idempotent. Closing a
pane with a live process follows an explicit user/automation policy rather than silently
discarding processes. Terminal byte streams bypass the workspace state store to avoid
re-rendering the entire UI for each output chunk.

## State and persistence
Model workspaces, tabs, pane trees, and sessions separately; a pane refers to a session.
Persist layout, titles, working directories, and allowed settings through versioned,
atomic snapshots. A restored local layout starts fresh shells: it does not claim to
restore killed processes. Remote sessions may reconnect through stable session IDs.
Do not persist terminal output or credentials by default. Store device credentials in
the OS keychain. Corrupt snapshots get a recoverable error and preserved backup.

## Automation contract
Use a user-private Unix socket with restrictive filesystem permissions and peer identity
checks. Same-user access is powerful and is not isolation from a malicious same-user
process. Expose explicit capabilities and document that limitation.

Version requests and responses, correlate them with request IDs, bound payload sizes,
and define error codes, deadlines, cancellation, and event subscription backpressure.
Separate read-only inspection from command execution. Define retry/idempotency behavior
before clients retry mutations. Never turn the local socket into an unauthenticated TCP API.

## Remote execution
Use the system SSH client initially to inherit host-key verification and existing user
configuration. Do not suppress host-key failures. Start/connect the remote agent over SSH;
only that agent allocates remote PTYs and launches remote commands. A broken connection
must never fall back to executing a remote command locally.

The daemon outlives the client connection and owns PTYs. Use bounded sequence-numbered
output replay, explicit truncation signals, resize messages, flow control, and idle/session
retention policies. Do not replay input automatically after an uncertain acknowledgment.
Define single-writer or explicit control handoff before supporting multiple clients.

## Optional cloud boundary
Keep identity and discovery separate from terminal traffic. Presence uses expiring leases;
it is not authoritative permission. Scope credentials to device/session and operation,
with short expiry and revocation. Design peer authentication and end-to-end encryption
before relaying terminal content; a relay should not gain terminal execution authority.
Use established cryptographic protocols/libraries rather than custom cryptography.

Start with one control service. Add a relay only after direct SSH requirements are met.
Use standard CI artifact storage rather than building a custom artifacts worker. Billing
is a later product decision and must not be required for local app functionality.

## Independent work
Agree on core actions, protocol schemas, adapter lifecycle, and storage format first.
Then UI, CLI, storage, and remote-agent work can proceed against original contract fixtures.
Each work item should declare owned paths, upstream dependencies, and acceptance behaviors.
Shared contracts need coordinated changes; module isolation does not establish license independence.
