# Zmux

Zmux is an independently implemented terminal workspace project: a Rust core with a thin
native Swift/AppKit macOS interface. Original Zmux code is MIT licensed.

Implementation has started with the portable workspace/action foundation. A native terminal
app, remote sessions and cloud services are still planned; no release is available yet.

## Development

Install Rust 1.95.0 with Cargo, rustfmt and Clippy, then run:

```sh
scripts/check
```

The Rust foundation can be checked on Linux. Native work initially targets macOS 14+ and
requires a macOS development machine; portable checks do not establish native UI behavior.

## Architecture and progress

- [Project plan](.agents/tasks/2026-10-14/01-build-zmux/plan.md)
- [Current implementation task](.agents/tasks/2026-10-14/02-foundation-prototype/plan.md)
- [Architecture](.agents/tasks/2026-10-14/01-build-zmux/references/architecture.md)
- [Session reliability](.agents/tasks/2026-10-14/01-build-zmux/references/session-reliability.md)
- [Dependency inventory](docs/dependencies.md)

## Licensing

See [LICENSE](LICENSE). Dependencies retain their own licenses; MIT for Zmux's original
code does not relabel third-party material. No cmux implementation or assets are bundled.
