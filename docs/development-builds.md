# macOS development builds

[macOS development build workflow](https://github.com/ashutoshpw/zmux/actions/workflows/macos-development.yml)

## Download and run

1. Open a successful workflow run and find its **Artifacts** section (GitHub sign-in required).
2. Download `Zmux-macos-dev-universal-<source-sha>`.
3. Extract the Actions artifact, then extract the inner `Zmux-dev-universal-<sha>.zip`.
4. Move **Zmux DEV** to your Applications folder and open it.

The bundle supports Apple Silicon and Intel Macs running macOS 14 or later. The artifact
also contains SHA-256 checksums, architecture/source metadata, native smoke results and a
rendered screenshot, plus a composited window capture when screen permissions permit.
Preserve the inner app ZIP when transferring it: it retains executable
permissions and bundle metadata that raw Actions file uploads do not preserve.

Artifacts expire after 14 days. To generate a fresh build, open the workflow above, select
**Run workflow**, and choose `main`. Changes to app, Rust, build scripts or this workflow
also start builds automatically when pushed to `main`.

This development build is ad-hoc signed and not notarized. macOS may require opening
**System Settings → Privacy & Security → Open Anyway** after the initial launch attempt.
Release signing, notarization and automatic updates are later distribution gates.

## What this development app includes

- Real local login shells managed by Rust session workers.
- Native AppKit terminal drawing with ANSI colors, cursor and wide-character grid positions.
- Workspace creation, renaming and switching; horizontal/vertical pane splits and closure.
- Keyboard input, arrow/control keys, an IME composition implementation, viewport selection,
  copy/paste and kernel PTY resize.
- A serialized command path and revisioned snapshots; native UI does not parse PTY output.

Use **⌘N** for a workspace, **⌘D** to split right, **⇧⌘D** to split down, and **⌘W** to close
the focused pane. Use the Edit menu for viewport copy, paste and select-all.

Initial limits: eight terminals per app context, 16 KiB per paste, 1,000 retained terminal
scrollback lines, and bounded command/input buffers. The first view displays the live
viewport; scrollback navigation, saved layouts, tabs, search, full terminal-protocol coverage,
remote SSH sessions, cloud authentication and iOS are not delivered by this build.
Interactive IME/accessibility behavior and resource budgets still need manual native dogfood.

## Build locally

On macOS with Xcode command-line tools, Rust 1.95, Python 3, and Cargo:

```sh
scripts/check
scripts/build-macos-dev
scripts/check-macos-app
```

The build script cross-compiles Rust and Swift for arm64 and x86_64, combines both executable
and runtime library, stages the bundle, ad-hoc signs it, verifies architectures/signatures,
and creates the ZIP with `ditto`. `--architecture host` produces only the local architecture.
The native check opens only the freshly packaged Zmux development bundle through Launch
Services; it does not operate on the user's cmux app.

## Workflow evidence

An artifact is uploaded only after portable/native Rust checks, universal compilation,
signing verification and packaged AppKit smoke checks pass. The smoke check drives a real
shell through native input, observes ANSI-colored output, splits panes, verifies `stty size`,
switches workspaces, closes a pane and captures an actual native rendering surface.
Those checks establish basic delivery, not complete UX parity or production readiness.
The hosted native smoke test runs on Apple Silicon. Intel executables are cross-compiled
and checked for architecture/signature, but Intel runtime behavior still needs dogfood.

## Verified build receipt

[Download the verified development artifact](https://github.com/ashutoshpw/zmux/actions/runs/38630706232/artifacts/11926235790)
from [successful run 38630706232](https://github.com/ashutoshpw/zmux/actions/runs/38630706232).
Source: `ee2599abc05b51f8459b7a0395c2bfa9e78c336f`; artifact expires 2026-10-28.

Inner ZIP SHA-256:

```text
efd9a594526c173efc12e4f92c7c848a3f29909b32e2f1cfd2f927fd418d89f0
```

The artifact was downloaded and checked for ZIP integrity, both binary architectures,
executable permissions, bundled notices and matching source metadata. All native smoke
conditions passed. WindowServer capture was unavailable on this runner; the cached viewport
image renders terminal output, but omits composited native button labels. Full visual
correctness remains a manual native dogfood gate.
