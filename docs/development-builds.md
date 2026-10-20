# macOS development builds

[macOS development build workflow](https://github.com/ashutoshpw/zmux/actions/workflows/macos-development.yml)

## Download and run

1. Open a successful workflow run and find its **Artifacts** section (GitHub sign-in required).
2. Download `zmux-macos-dev-universal-<source-sha>`.
3. Extract the Actions artifact, then extract the inner `zmux-dev-universal-<sha>.zip`.
4. Move **zmux** to your Applications folder and open it.

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
- Workspace creation, renaming and switching; horizontal/vertical pane splits and closure.
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

Use **⌘N** for a workspace, **⌘T** for a terminal tab, **⌘D** to split right, **⇧⌘D** to
split down, **⌘W** to close the selected tab, and **⇧⌘T** to reopen the last closed item.
Use **Ctrl+Tab** / **Ctrl+Shift+Tab** for the next/previous tab in the focused pane, wrapping
at either end. **⇧⌘[** / **⇧⌘]** also traverse tabs. With a single tab, the Control shortcuts
do nothing and do not send input to the terminal. Use History → Recently Closed to choose an
item to reopen. Use the globe icon or Workspace → New Browser Tab to browse, and ⌘L to focus the browser
address bar. Use the Browser menu for page find/zoom/navigation. Use the Edit menu for copy/paste.

Use Shift-PageUp/PageDown for history pages and Command-Home/End for oldest/live output.
Hold Shift while scrolling to bypass application mouse tracking.

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

[Download the verified universal development build](https://github.com/ashutoshpw/zmux/actions/runs/38647522249/artifacts/11932279010)
from [successful run 38647522249](https://github.com/ashutoshpw/zmux/actions/runs/38647522249).
Source: `7e9355b4dd820580443777ae6b3dd9a5fb0f4024`; expires 2026-10-28.

Inner ZIP SHA-256:

```text
af702ab35d18da35de71421eed0b48477a61f3e0b67a2f548aa47db2b02b897e
```

The downloaded archive was checked for integrity, checksum, matching metadata, executable
permissions, notices/icon and both architectures in the app, CLI and runtime library. Native
input/rendering, pane/workspace operations, terminal tab create/close/reopen, surviving PIDs,
sidebar/attention routing, WebKit form/state retention and balanced/retained split geometry passed.

Both captured images were inspected. `Zmux-native-smoke.png` shows AppKit chrome and balanced
panes; `zmux-browser-smoke.png` shows actual WebKit page pixels and retained form input. Cached
AppKit rendering omits remote WebContent layers; WindowServer capture was unavailable on this
runner. Physical keyboard/drag/IME/VoiceOver, Intel runtime and measured resource/remote-soak
budgets still require native dogfood. Cloud callback smoke is offline, and the service remains
undeployed. See the [detailed delivery receipt](../.agents/tasks/2026-10-14/05-command-ux-parity/references/native-delivery.md).
