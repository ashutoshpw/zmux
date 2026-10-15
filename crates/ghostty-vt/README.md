# Ghostty VT foreign owner

This original MIT crate links the exact accepted static engine through one audited
C boundary. It exposes worker-bound safe Rust values, with no `Send`, raw handles,
callback PTY writes, second parser or fallback engine. Core/runtime crates continue
to forbid unsafe Rust. The normal workspace build and existing `Terminal` remain
unchanged; the `linked` feature is enabled only by terminal-adapter's explicit
`ghostty` feature.

Set `ZMUX_GHOSTTY_ARTIFACT_ROOT` to an already retained, reviewed artifact root.
The build runs `tools/terminal-engine-artifacts/accept-linkage.py`, checks exact
library/producer/source/header/notice identities and compiles only the original
small C host shim. It never downloads, searches system libraries or invokes Zig.
Linux uses the retained reconstruction root; Darwin uses the root containing
`original-upload`, `supplemental-source` and `independent-review.json`. Both thin
Darwin slices are pinned. The required workflow source supplement is checked
separately; the original producer receipt is never rewritten. The library declares
macOS 13; new consumers require explicit `MACOSX_DEPLOYMENT_TARGET=14.0`
and their C shim targets macOS 14. This does not establish runtime behavior
on macOS 13 or 14.

Example focused validation from the repository root:

```sh
ZMUX_GHOSTTY_ARTIFACT_ROOT="$PWD/target/terminal-engine-artifacts/20261015-publication-boundary" \
  cargo test -p zmux-terminal-adapter --features ghostty --test staged_engine
```

The C translation unit reuses the frozen reviewed Resource, Integrated, collector,
metadata and cell exporter inputs unchanged. It generalizes constructor geometry
and caller-supplied mapping caps, and performs full-cell export before the existing
quota recheck/export seal. The two fixture-local `reply` symbols are explicitly
renamed around the Probe include. The scratch-text exporter remains unused.

The safe adapter stages a complete existing `Viewport`, title/cwd/notices and a
bounded immutable reply batch. No next engine mutation is allowed before commit.
The worker must finish previous admitted bytes and causal reply physical writes
through its sole PTY writer, suppress output polling/input admission while staged,
and only then commit the exact generation. Cancellation, deadline, partial write
or resize failure must abort permanently and retain the old published viewport.
Engine resize precedes kernel PTY resize; failure is visible, with no rollback or
atomicity claim. Zero-reply stages still need commit. These are caller obligations;
this slice does not wire the session worker or enable a default engine switch.

Host buffers are fixed-capacity before C construction: 28,800 cells, 512 KiB text,
1,024 events and metadata records, 4,096 reply bytes, 20,480 raw metadata bytes and
two normalized metadata slots. Public `Viewport` conversion uses fallible reserves,
with dimensions/history and exported text bounded; retained caller clones are
outside the owner bound. Explicit aggregate/general/page caps account engine-owned
mappings only. Fixture settings are not product budgets, total RSS or performance
acceptance. Extended styles, hyperlinks and dynamic palette changes fail visibly
because the existing `Viewport` cannot represent them. Typed input, selection,
IME, native rendering/workloads, worker publication/close integration and production
policy remain acceptance gates. Existing third-party notice inventories cover the
retained engine; release packaging must retain them.
