# Proposed architecture

## Product boundary
Zmux is a native terminal workspace app with a command-line automation interface.
The first release provides local shells, tabs, pane splits, workspace switching,
layout restoration, and explicit notifications for agent workflows. Remote execution
is a separate capability. Cloud accounts, billing, browser embedding, collaboration,
and iOS are outside the initial local MVP.

## Stack decision
Use Rust for reusable workspace, session, storage, transport, and automation logic. Keep
the macOS shell thin and native in Swift/AppKit; SwiftUI is suitable for settings and
other low-frequency views. This choice favors macOS integration while sharing Rust logic
with the CLI and remote daemon. It is not evidence of lower CPU/memory use or a fix for
the user's cmux hangs. Measure both resource use and responsiveness in the early prototype.

egui/eframe and PhotoCraft's UI stack are deferred, not dependencies of the initial app.
A Windows/Linux UI can later consume the same Rust core if cross-platform desktop support
becomes a priority. Keep platform UI types out of that core. Do not import PhotoCraft's
image-processing code or assume its desktop behavior establishes terminal performance.
The evaluated reference is [PhotoCraft at a62ef68](https://github.com/storytold/photocraft/tree/a62ef68ca2495b680634e0421b702e4de88287a6):
its module separation informs the design discussion; Zmux's native UI/terminal stack is its own decision.

## Current implementation boundary
The [foundation task](../.agents/tasks/2026-10-14/02-foundation-prototype/plan.md) implements the Rust action
model, headless PTY/parser prototype, development CLI and model-only C ABI. The proposed
native coordinator, Swift wrapper, renderer, storage and remote/cloud clients remain pending.
The parser is a feasibility candidate rather than the finalized terminal engine.

## Monorepo boundaries and ownership
Paths below are proposals, not existing packages. Each row can become an independently
owned workstream after its upstream contract is defined. Start with one repository;
publish separate artifacts only when a concrete consumer needs them.

| Proposed path | Responsibility | Dependencies / proposed technology |
| --- | --- | --- |
| `crates/workspace-core/` | Stable IDs, split tree, actions, state transitions, effects | Rust; no AppKit, PTY I/O, or networking |
| `crates/terminal-adapter/` | Reviewed engine abstraction and immutable visible-screen snapshots | Rust-facing adapter; engine need not itself be Rust |
| `crates/session-runtime/` | Local PTY lifecycle, per-session workers, bounded queues, cleanup | Rust; integrates engine ownership without duplicate PTYs |
| `crates/workspace-storage/` | Versioned snapshots, atomic saves, migrations | Rust; core serialization contracts |
| `packages/automation-protocol/` | Schemas, versioning rules, error codes, original fixtures | Language-neutral JSON schemas |
| `crates/local-control/` | Socket lifecycle, peer validation, action dispatch | Rust; protocol and core |
| `crates/native-bridge/` | Commands, events, snapshot access, opaque handle lifecycle | Small Rust/Swift boundary; tooling selected in M0 |
| `packages/NativeBridge/` | Swift wrapper and event delivery into native views | Swift; bridge ownership rules |
| `apps/macos/` | Native windows, panes, terminal presentation, accessibility, settings | Thin Swift/AppKit shell; SwiftUI where suitable |
| `apps/cli/` | CLI parsing and API client | Rust; no independent business rules |
| `services/remote-agent/` | Remote PTY ownership, stream replay, session cleanup | Rust; Linux first; reuse eligible session/protocol crates |
| `crates/remote-transport/` | SSH connection lifecycle and stream framing | Rust client; documented wire protocol |
| `services/control-plane/` | Device registry, identity, presence, optional relay grants | TypeScript; PostgreSQL proposed |
| `services/relay/` | Authenticated forwarding with quotas | Choose runtime after transport feasibility |
| `apps/web/` | Optional device/account UI | TypeScript; control-plane API |
| `apps/ios/` | Remote session presentation and input | Swift native UI; reuse Rust client core where feasible |

## Shared action flow
UI, shortcuts, and local automation submit typed actions to a workspace coordinator.
The core validates actions and computes state changes plus explicit effects. Effect
handlers own PTYs, persistence, notifications, and networking. Asynchronous completion
returns typed events to the coordinator. Serial state mutation prevents races between
UI and automation; effects must not block that executor.

The coordinator lives in Rust, off the AppKit main thread. Swift submits commands and
observes immutable results; it does not implement a second workspace mutation model.
Per-session workers own ordered input/output processing. Blocking reads and subprocess
waits use dedicated I/O facilities, not the UI thread or the coordinator executor.

Example actions: create workspace, split pane, focus pane, close session. Actions
carry stable object IDs and request context. Session closure is idempotent. Closing a
pane with a live process follows an explicit user/automation policy rather than silently
discarding processes. Terminal byte streams bypass the workspace state store to avoid
re-rendering the entire UI for each output chunk.

## Native bridge and rendering
Prototype a small explicit bridge during M0 before selecting generated bindings or a C ABI.
Specify ownership of opaque handles/buffers, version negotiation, callback threading,
error mapping, cancellation, and destruction. No panic may unwind across the FFI boundary.
Report failures explicitly; disposing a view must detach its callbacks and invalidate stale
handles without synchronously waiting on network shutdown from the main thread.

Use asynchronous commands/events for session work. Deliver coalesced UI events on the
main thread, with generation IDs to discard completions from closed/replaced sessions.
No core lock may be held while invoking Swift callbacks, and a UI snapshot read must not
wait on PTY/network activity. Avoid copies of complete scrollback across FFI per frame;
expose bounded visible-screen data or reviewed engine drawing handles with explicit lifetime.

AppKit owns native views and main-thread-only rendering calls. The upstream engine may
own GPU resources or require its own rendering thread; verify that contract rather than
assuming all engine operations can move to Rust workers. M0 must prove that engine/PTY
ownership, thread requirements, native drawing, and input mapping fit this architecture.
The chosen engine may be Rust, C, or Zig behind an adapter; do not rewrite a mature engine
just to achieve a single-language label.

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

Use explicit connecting, active, reconnecting, closing, closed, and failed states, with a
generation per connection attempt. Bound retry frequency and cancel superseded attempts.
A local detached client cannot leak workers; a remote daemon retains sessions only under
its documented policy. See [session-reliability.md](../.agents/tasks/2026-10-14/01-build-zmux/references/session-reliability.md) for limits,
fair scheduling, output integrity, diagnostics, and required native stress evidence.

Start with worker isolation in the app and a separate remote daemon. A local session-helper
process is an optional escalation if crash containment or profiling justifies its IPC and
packaging costs. Do not equate threads or async tasks with process-level crash isolation.

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
Agree on core actions, protocol schemas, adapter/FFI lifecycle, and storage format first.
Then UI, CLI, storage, and remote-agent work can proceed against original contract fixtures.
Each work item should declare owned paths, upstream dependencies, and acceptance behaviors.
Shared contracts need coordinated changes; module isolation does not establish license independence.
