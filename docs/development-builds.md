# macOS development builds

[macOS development build workflow](https://github.com/ashutoshpw/zmux/actions/workflows/macos-development.yml)

## Download and run

1. Open a successful workflow run and find its **Artifacts** section (GitHub sign-in required).
2. Download `zmux-macos-dev-universal-<source-sha>`.
3. Extract the Actions artifact, then extract the inner `zmux-dev-universal-<sha>.zip`.
4. Quit any older Zmux instance, move **zmux** to your Applications folder, then open it.

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
- Wheel/trackpad and keyboard history navigation, anchored output and post-exit browsing;
  typing/paste returns to live output. Mouse-aware applications receive negotiated wheel bytes.
- Workspace creation, renaming and switching; drag workspaces to reorder them within a window;
  horizontal/vertical pane splits and closure.
- Independent terminal tabs, selection/traversal, renaming, drag between panes, and close-other.
- In-process Recently Closed history with retryable restoration of tabs and grouped layouts.
- Collapsible sidebar (⌘B), Workspaces/Notifications tabs, unread workspace/tab badges and an
  always-visible bell control and Dock unread count. Inbox entries open their exact terminal,
  mark read or dismiss; tab focus/window activation acknowledges that focused surface.
- Bell, OSC 9 and OSC 777 notifications, plus explicit `zmux notify`; plain output does not notify.
- Embedded system WebKit browser tabs with address bar, back/forward/reload/stop, find and zoom.
  Browser views retain in-memory page state when hidden and do not allocate PTYs.
- Keyboard input, arrow/control keys, an IME composition implementation, viewport selection,
  copy/paste and kernel PTY resize.
- Accessible icon controls, an original app icon, and active terminal titles from OSC metadata.
- Bundled `zmux --help` and JSON automation commands over a private same-user Unix socket.
- Browser cloud sign-in with PKCE, Keychain credentials and account/device management.
- A serialized command path and revisioned snapshots; native UI does not parse PTY output.

The default `vt100` engine's OSC 7501 Programs interface passed exact-source packaged
macOS acceptance with literal shell reports sent through the app PTY and production
parser. The accepted app uses `vt100`; optional Ghostty is not linked into this build.
Separate retained Ghostty linkage tests cover its 7501 engine and Session paths, but do
not change the delivered app's engine. See the [program status guide](program-status.md)
and the [verified build receipt](#verified-build-receipt).

Use **⌘N** for a workspace, **⌘T** for a terminal tab, **⌘D** to split right, **⇧⌘D** to
split down, **⌘W** to close the selected tab, and **⇧⌘T** to reopen the last closed item.
Use **Ctrl+Tab** / **Ctrl+Shift+Tab** for the next/previous tab in the focused pane, wrapping
at either end. **⇧⌘[** / **⇧⌘]** also traverse tabs. With a single tab, the Control shortcuts
do nothing and do not send input to the terminal. Use History → Recently Closed to choose an
item to reopen. Use the globe icon or Workspace → New Browser Tab to browse, and ⌘L to focus the browser
address bar. Use the Browser menu for page find/zoom/navigation. Use the Edit menu for copy/paste.

Use Shift-PageUp/PageDown for history pages and Command-Home/End for oldest/live output.
Hold Shift while scrolling to bypass application mouse tracking.
To copy visible text, drag across it and press **Cmd+C**, including when tmux or
another application has enabled mouse tracking. Mouse controls continue to reach
the application. Hold Shift before dragging to force local selection instead.
**Ctrl+C** continues to send an interrupt to the running program.

Initial limits: 32 terminals and four browser tabs per app context, 16 KiB per paste, 1,000 retained terminal
scrollback lines, and bounded command/input buffers. Saved layouts, search, full terminal-protocol coverage,
remote SSH sessions and iOS are not delivered by this build. Cloud browser sign-in and
account/device management are integrated, but production service deployment remains pending
the hosting discussion. This does not yet provide remote terminals.
Closed-item history holds up to 32 items and lasts until app exit. Reopening starts fresh
local shells at saved working directories; it never replays commands or resurrects killed jobs.
A missing directory leaves a visible error and retryable history. Workspace closures preserve
the grouped split layout. A removed individual pane currently restores into a new workspace;
restoring its original split position and multiwindow recovery remain open parity gaps.
Interactive drag/IME/accessibility behavior and resource budgets still need manual native dogfood.

## Command line control

Inside a zmux terminal, run `zmux --help` or `zmux help send-text`. To use the CLI from
other terminals, choose **zmux → Install Command Line Tool…** and select a writable
directory on your PATH. The installer creates a symlink without overwriting an existing
file. Keep the app at its installed location.

Examples: `zmux status`, `zmux list-workspaces`, `zmux list-panes`, and
`zmux new-workspace "Build"`. For tabs, use `zmux new-surface PANE_ID`,
`zmux list-pane-surfaces PANE_ID`, `zmux select-surface SURFACE_ID`,
`zmux close-surface SURFACE_ID`, `zmux recently-closed`, and `zmux reopen-closed [CLOSED_ID]`.
Mutations acknowledge acceptance; inspect tab readiness for asynchronous shell launch results.
For an embedded browser use `zmux browser open PANE_ID https://example.com` and
`zmux browser navigate SURFACE_ID https://example.com/docs`.
IDs currently use positive integers; cmux-style refs, UUIDs and option parity remain pending.
For attention, run `zmux notify --surface SURFACE_ID --title "Review ready" --body "Build finished"`
and `zmux list-notifications`. Use `open-notification --id ID`, `mark-notification-read --id ID`,
`dismiss-notification --id ID`, or `clear-notifications --through ID` to acknowledge exact history.
Browser tabs use private, per-tab WebKit storage. Persistent/shared profiles, downloads,
permissions/dialog workflows and browser automation remain pending. Only http/https/about:blank
are supported; local files, credential URLs and privileged schemes are rejected. The app permits
user-selected HTTP web content while preserving default TLS certificate validation and the
cloud client's HTTPS requirement.
The inbox retains 128 entries and reports omissions; desktop OS notifications, shell/agent hooks,
status/progress/log operations and persisted unread state remain later work.
Each command has its own offline `--help`.

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
The native check opens only the freshly packaged zmux development bundle through Launch
Services; it does not operate on the user's cmux app.

## Workflow evidence

An artifact is uploaded only after portable/native Rust checks, universal compilation,
signing verification and packaged AppKit smoke checks pass. The smoke check drives a real
shell through native input, observes ANSI-colored output, splits panes, verifies `stty size`,
switches workspaces, closes a pane, exercises ⌘T/⌘W/⇧⌘T through the native menu,
checks that the sibling PID survives, and captures an actual native rendering surface.
Those checks establish basic delivery, not complete UX parity or production readiness.
The hosted native smoke test runs on Apple Silicon. Intel executables are cross-compiled
and checked for architecture/signature, but Intel runtime behavior still needs dogfood.

## Verified build receipt

[Download the verified universal development build](https://github.com/ashutoshpw/zmux/actions/runs/43455598200/artifacts/14057620449)
from [successful run 43455598200](https://github.com/ashutoshpw/zmux/actions/runs/43455598200).
The run built exact source `2c0cf2b40c4f0ee04048209272e3172a1ac30ce1`; the artifact expires
2026-11-27 16:21:25 UTC. GitHub sign-in may be required.

- The downloaded Actions artifact is **30,775,347 bytes**, SHA-256
  `3c0a077e41f71fd56465f7c4e2fe8c2a47f32d33582e473f4a2bee44ad40c1c4`.
- Its inner app ZIP is **26,916,243 bytes**, SHA-256
  `3da3a1a57243550c791e2146d721b3cf674ebbc256bb24ff5b0aa0e90fd805eb`.
- Verified app, CLI, native bridge and scheduler contain **arm64 and x86_64** slices;
  packaged metadata and `Info.plist` declare **macOS 14.0**. The package is ad-hoc
  signed and not notarized.
- Default terminal engine: `vt100`. Optional Ghostty is not linked into this app.

Local `scripts/check` passed **767/0/7** in 262.30 seconds with unchanged source and
unrelated work preserved. Exact-source Foundation run [43455639671](https://github.com/ashutoshpw/zmux/actions/runs/43455639671)
passed Ubuntu **767/0/7** and macOS **765/0/7**; the universal workflow's integrated
check passed **765/0/7**. Hosted native acceptance passed for terminal selection,
sidebar, Programs and system status menu. Its Programs fixture sent literal OSC 7501
shell `printf` reports through the packaged PTY and production parser; exact owner,
record, selection, inventory and provider-coexistence assertions passed.

The parent reviewed the actual compact Programs and expanded Agents screenshots from
the downloaded package. The Programs image uses a scrolled viewport. The expanded
Agents image uses a temporary fixture viewport and does not show every row fitting the
normal 28% sidebar height allocation. The idle/working fixture requested no alert sound, so audio
playback was not tested.

Separate retained Ghostty linkage run [43407437492 / artifact 14027538458](https://github.com/ashutoshpw/zmux/actions/runs/43407437492/artifacts/14027538458)
is pinned to source `2efb76d9bcbb9e0a00015602b8bcaf81c00920de`. Its 229/229 source
inputs matched the delivered source pins; both Apple targets compiled and linked, and
the ARM engine **4/4**, linked 7501 engine **1/1**, Session **4/4**, and real-PTY
7501 Session **1/1** tests passed. This retained evidence does not mean Ghostty is in
the app package and does not establish Intel runtime behavior.

Native acceptance ran on hosted Apple Silicon. Intel runtime, physical-user acceptance
and production behavior are not claimed. See the task's [permanent delivery record](../.agents/tasks/2026-11-12/01-osc-7501-support/references/delivery.md)
and [hosted run evidence](../.agents/tasks/2026-11-12/01-osc-7501-support/references/hosted-final-2c0cf2b.md).
