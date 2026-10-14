# Polished zmux desktop, CLI and cloud integration

**Date:** 2026-10-14
**Task:** 04-polished-ui-cloud

## Objective
Deliver a polished lowercase `zmux` native development app with a generated icon,
useful bundled CLI and real account/device cloud integration. Publish a verified
macOS artifact; all cloud deployments must execute through GitHub Actions.

## Context
- User supplied current preview and cmux UI references and explicitly requested implementation.
- Original MIT implementation only; no copied or translated cmux GPL/BUSL implementation/assets.
- Cloud origin is `https://zmux.w3dev.app`; user deferred hosting selection to a separate discussion.
- Existing native prototype and Actions build are working; full roadmap remains broader.

## Approach
Implement compact native workspace navigation and icon controls; expose active terminal
metadata. Add private Unix socket automation and a packaged universal CLI. Build a
Better Auth/PostgreSQL control service with native S256 PKCE and scoped device management;
store native credentials in Keychain. Deploy only through protected Actions workflows.

## Steps
- [ ] Native window/sidebar/pane polish, lowercase branding and active terminal titles.
- [ ] Generate original icon and package app icon plus universal CLI.
- [ ] Implement CLI help and real app control through a bounded same-user socket.
- [ ] Implement real cloud service, auth/device APIs and disposable-database checks.
- [ ] Integrate native browser sign-in, Keychain, account/device UI and configuration.
- [ ] Add validated Actions cloud packaging and document protected deployment handoff.
- [ ] Deploy to the selected host through Actions after the deferred hosting discussion.
- [ ] Validate cohesive slices, commit on main and publish tested macOS artifact.
- [ ] Obtain fresh read-only review and record exact evidence/remaining gates.

## Acceptance criteria
- [ ] Native UI has accessible symbol controls, clear workspace selection and terminal title.
- [ ] Icon appears in bundle metadata; installed/bundled `zmux --help` prints useful help.
- [ ] CLI and UI use the same Rust actions and protect socket ownership/size/time boundaries.
- [ ] Real browser/native auth validates state/PKCE, single-use codes and session revocation.
- [ ] Device APIs enforce user ownership and disclose unavailable remote/relay capabilities.
- [ ] Cloud deployment runs only via Actions and reports actual healthy deployed origin.
- [ ] Hosted native build/smoke and downloaded artifact checks pass for delivered source.

## References
- [Implementation evidence](references/implementation.md) — ownership, contracts and validation.
