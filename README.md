# zmux

zmux is an independently implemented terminal workspace project: a Rust core with a thin
native Swift/AppKit macOS interface. Original zmux code is MIT licensed.

The native development app connects the Rust workspace/session core to Swift/AppKit.
It provides local shells, workspaces, split panes and a bundled automation CLI. The cloud
client and identity/device service are implemented; production hosting is pending. Remote
terminal transport, mobile clients and full terminal UX parity remain in development.

[Download a macOS development build](docs/development-builds.md) from a successful GitHub
Actions run. Universal app compilation, packaged native smoke checks and artifact upload
are required before a build is offered; this is an ad-hoc signed development app.

## Development

Install Rust 1.95.0 with Cargo, rustfmt and Clippy, then run:

```sh
scripts/check
```

The Rust foundation can be checked on Linux. Native work initially targets macOS 14+ and
requires a macOS development machine; portable checks do not establish native UI behavior.

## Try the headless terminal prototype

```sh
cargo run --locked -p zmux -- prototype -- /bin/sh -c 'printf "Hello from zmux\n"'
```

This runs a local command in a real PTY and returns its final visible screen and exit status
as JSON. It does not open a native window, retain the full output transcript, or connect
remotely. It times out after 10 seconds; set `--timeout SECONDS` before `--` when needed.
See [prototype limits](docs/terminal-prototype.md).

## Terminal history and attention

Scroll with the mouse wheel or trackpad. Shift-PageUp/PageDown moves a page;
Command-Home/End jumps to the oldest retained output or back to the live screen.
Typing or pasting returns to live output. History retains up to 1000 lines and remains
available after a process exits. Mouse-aware full-screen applications receive wheel input;
hold Shift to request history navigation instead.

BEL, OSC 9, OSC 777 and `zmux notify` feed the Notifications inbox and unread badges,
including the Dock badge. Selecting a surface or reactivating its window acknowledges
that focused surface's existing attention. Opening an inbox entry acknowledges that exact
notification. Ordinary terminal output alone does not generate attention. OS notification
banners, agent completion hooks and full command parity remain in development.
See the [CLI parity repair plan](.agents/tasks/2026-10-14/05-command-ux-parity/references/scroll-attention-repair.md).

## Cloud

The intended origin is `https://zmux.w3dev.app`. Deployment runs through GitHub Actions
only, after hosting selection. See [deployment status and handoff](docs/cloud-deployment.md)
and the [service contract](services/control-plane/README.md).

## Architecture and progress

- [Project plan](.agents/tasks/2026-10-14/01-build-zmux/plan.md)
- [Native development build task](.agents/tasks/2026-10-14/03-macos-development-build/plan.md)
- [Completed portable foundation](.agents/tasks/2026-10-14/02-foundation-prototype/plan.md)
- [Architecture](.agents/tasks/2026-10-14/01-build-zmux/references/architecture.md)
- [Session reliability](.agents/tasks/2026-10-14/01-build-zmux/references/session-reliability.md)
- [Native bridge contract](docs/native-bridge.md)
- [Dependency inventory](docs/dependencies.md)

## Licensing

See [LICENSE](LICENSE) and [third-party notices](THIRD_PARTY_NOTICES.md). Dependencies retain their own licenses; MIT for zmux's original
code does not relabel third-party material. No cmux implementation or assets are bundled.
