# Contracts and evidence

## Ownership
- Native UI worker: App.swift, Models.swift, WorkspaceViews/UIComponents and visual terminal styling.
- CLI worker: CLI, desktop runtime/control listener, native bridge and Rust dependencies.
- Cloud worker: services/control-plane, cloud validation workflow and check script.
- Parent: original icon, terminal title metadata, native cloud client, packaging, deployment workflow and integration.

## Native cloud contract
Origin is an explicit owned HTTPS service (loopback HTTP permitted for development).
Client `zmux-macos-dev`; callback `zmux-dev://oauth/callback`. Browser authorization uses
state and S256 PKCE at `/v1/native/authorize`, code exchange `/v1/native/token`.
Authenticated `/v1/me`, `/v1/devices`, heartbeat, revoke use bearer headers.
No credentials in callback URLs. Native token storage uses macOS Keychain.

## Test owners
- CLI executable plus real desktop socket owns help, shared action behavior and transport bounds.
- Cloud service with real disposable PostgreSQL owns authorization, code exchange and cross-user access.
- Native packaged smoke owns Swift/C linkage, title propagation, icon/bundled CLI and UI construction.
- Existing Rust tests remain owners of workspace/session/parser invariants; avoid redundant fixtures.
