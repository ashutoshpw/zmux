# Zmux

Zmux is an independently implemented terminal workspace project: a Rust core with a thin
native Swift/AppKit macOS interface. Original Zmux code is MIT licensed.

The native development app connects the Rust workspace/session core to Swift/AppKit.
It provides local shells, workspaces and split panes. Remote sessions, cloud services,
mobile clients and full terminal UX parity remain in development.

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
cargo run --locked -p zmux -- prototype -- /bin/sh -c 'printf "Hello from Zmux\n"'
```

This runs a local command in a real PTY and returns its final visible screen and exit status
as JSON. It does not open a native window, retain the full output transcript, or connect
remotely. It times out after 10 seconds; set `--timeout SECONDS` before `--` when needed.
See [prototype limits](docs/terminal-prototype.md).

## Architecture and progress

- [Project plan](.agents/tasks/2026-10-14/01-build-zmux/plan.md)
- [Native development build task](.agents/tasks/2026-10-14/03-macos-development-build/plan.md)
- [Completed portable foundation](.agents/tasks/2026-10-14/02-foundation-prototype/plan.md)
- [Architecture](.agents/tasks/2026-10-14/01-build-zmux/references/architecture.md)
- [Session reliability](.agents/tasks/2026-10-14/01-build-zmux/references/session-reliability.md)
- [Native bridge contract](docs/native-bridge.md)
- [Dependency inventory](docs/dependencies.md)

## Licensing

See [LICENSE](LICENSE) and [third-party notices](THIRD_PARTY_NOTICES.md). Dependencies retain their own licenses; MIT for Zmux's original
code does not relabel third-party material. No cmux implementation or assets are bundled.
