# Dependency and provenance inventory

Original Zmux implementation is MIT. Runtime dependencies may use reviewed compatible
permissive licenses; retain upstream notices rather than relabeling them as MIT.
`Cargo.lock` pins the complete Rust dependency graph. No third-party fonts/assets are bundled.

| Direct dependency | Purpose | Declared license | Source |
| --- | --- | --- | --- |
| `serde` | Public model/command serialization | MIT OR Apache-2.0 | <https://github.com/serde-rs/serde> |
| `serde_json` | Protocol/bridge JSON (reserved for subsequent slice) | MIT OR Apache-2.0 | <https://github.com/serde-rs/json> |
| `thiserror` | Explicit domain errors | MIT OR Apache-2.0 | <https://github.com/dtolnay/thiserror> |

Resolved package versions and full license material will be inventoried before the first
distribution. Review native bundles and transitive assets separately from Rust manifests.
This is a source prototype, not a completed distributable license audit.

## Implementation sources

Workspace/action implementation and behavioral fixtures were authored for Zmux from its
own plan. No cmux source, tests, assets or implementation patches were imported. Prior
discussion referenced cmux licensing and PR #1 behavior; this is not a strict clean-room claim.

## Resolved Rust packages (slice 1)

| Package | Version | Declared license |
| --- | --- | --- |
| `proc-macro2` | 1.0.107 | MIT OR Apache-2.0 |
| `quote` | 1.0.47 | MIT OR Apache-2.0 |
| `serde` | 1.0.229 | MIT OR Apache-2.0 |
| `serde_core` | 1.0.229 | MIT OR Apache-2.0 |
| `serde_derive` | 1.0.229 | MIT OR Apache-2.0 |
| `syn` | 3.0.9 | MIT OR Apache-2.0 |
| `thiserror` | 2.0.21 | MIT OR Apache-2.0 |
| `thiserror-impl` | 2.0.21 | MIT OR Apache-2.0 |
| `unicode-ident` | 1.0.27 | (MIT OR Apache-2.0) AND Unicode-3.0 |
