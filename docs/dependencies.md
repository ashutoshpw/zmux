# Dependency and provenance inventory

Original Zmux implementation is MIT. Runtime dependencies may use reviewed compatible
permissive licenses; retain upstream notices rather than relabeling them as MIT.
`Cargo.lock` pins the complete Rust dependency graph. No third-party fonts/assets are bundled.

| Direct dependency | Purpose | Declared license | Source |
| --- | --- | --- | --- |
| `serde` | Public model/command serialization | MIT OR Apache-2.0 | <https://github.com/serde-rs/serde> |
| `serde_json` | Prototype output and bridge JSON | MIT OR Apache-2.0 | <https://github.com/serde-rs/json> |
| `portable-pty` 0.9.0 | Unix PTY allocation/process launch | MIT | <https://github.com/wezterm/wezterm> |
| `vt100` 0.16.2 | Headless parser feasibility candidate | MIT | <https://github.com/doy/vt100-rust> |
| `filedescriptor` 0.8.3 | Safe descriptor duplication/nonblocking I/O/poll | MIT | <https://github.com/wezterm/wezterm> |
| `socket2` 0.6.3 | Safe Darwin wake-socket SIGPIPE suppression | MIT OR Apache-2.0 | <https://github.com/rust-lang/socket2> |
| `nix` 0.28.0 | Terminate owned Unix PTY process groups | MIT | <https://github.com/nix-rust/nix> |
| `thiserror` | Explicit domain errors | MIT OR Apache-2.0 | <https://github.com/dtolnay/thiserror> |

Resolved package versions and available license texts are inventoried and bundled in the
development app. Review native bundles and transitive assets separately from Rust manifests
before release; the development inventory is not a completed release license audit.

## Implementation sources

Workspace/action implementation and behavioral fixtures were authored for Zmux from its
own plan. No cmux source, tests, assets or implementation patches were imported. Prior
discussion referenced cmux licensing and PR #1 behavior; this is not a strict clean-room claim.

## Resolved versions and notices

[THIRD_PARTY_NOTICES.md](../THIRD_PARTY_NOTICES.md) inventories all resolved packages,
including target-specific packages and available top-level license files. Generate it with
`scripts/license-inventory`; `scripts/check` detects changes against the locked graph.
The compiler dependency `unicode-ident` additionally requires Unicode-3.0 notices.
Compatible permissive licensing does not mean every dependency is exclusively MIT.

## Prototype source review

Registry API/source review established that portable-pty's Unix backend creates a new
session/process group; Zmux independently owns nonblocking I/O and group shutdown.
vt100 0.16.2 has bounded cells/scrollback, but its vte dependency enables a growable OSC
buffer. The original Zmux adapter therefore rejects oversized control strings explicitly.
The parser is an experimental headless candidate, not a finalized native engine choice.
No upstream source was copied into original Zmux implementation; upstream license texts
are retained verbatim in the notice artifact.
