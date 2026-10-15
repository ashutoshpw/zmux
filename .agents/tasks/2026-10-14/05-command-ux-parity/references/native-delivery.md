# Accepted native development build

- Source: `70cd3b614e0b3f9cf0fcaa0e33bec4c59c228d7a`.
- [Successful macOS workflow](https://github.com/ashutoshpw/zmux/actions/runs/38692821187).
- [Download universal development artifact](https://github.com/ashutoshpw/zmux/actions/runs/38692821187/artifacts/11952308942).
- Artifact expires 2026-10-29 06:33 UTC. Inner ZIP: `zmux-dev-universal-70cd3b6.zip`.
- SHA-256: `f75cef6e14fe7bae9633c1870908b20b0ef85dafc75ab98eb987f3303eec1148`.
- 4,981,755-byte ZIP downloaded and checked for integrity, checksum, matching app/build/smoke
  source metadata, executable permissions, icon/notices and arm64/x86_64 architectures in the
  app executable, CLI and Rust library. Hosted signing verification passed; signing is ad hoc.

## Executed evidence

`scripts/check` passed locally on Linux and inside this macOS workflow. The packaged application
was launched through Launch Services on the Apple Silicon runner. Native smoke passed:

- ANSI rendering, native input, kernel PTY resize, workspace switching and pane cleanup.
- ⌘T, ⌘W and ⇧⌘T through native menu equivalents; closed history must be consumed and the
  surviving terminal PID must remain unchanged. Synthetic events preserve Shift and use actual
  ANSI keycodes; dynamic menu availability is refreshed. Physical keyboard dogfood remains open.
- Wheel history navigation through a native CGEvent, Command-Home/End, native accessibility
  contents update and typed-input return to live output.
- Sidebar toggle/layout, actual hidden-PTY OSC signal → notification inbox → exact target/read;
  tab selection acknowledges existing attention and a later hidden-tab signal stays unread.
- Real WebKit document/form manipulation and hidden-tab state retention.
- Split ratio adjustment survives tab switching; balanced browser geometry is asserted.
- Active terminal title, accessible icon labels, cloud callback contract and bundled CLI control.

Downloaded `Zmux-native-smoke.png` (45,161 bytes) was inspected: evenly sized panes, clean terminal,
terminal/browser tabs, sidebar controls, icon actions and cloud footer. Downloaded
`zmux-browser-smoke.png` (14,316 bytes) was inspected: actual WebKit page pixels with the retained
form value. Cached AppKit capture omits remote WebContent layers, so these are separate images.
WindowServer capture returned status 1; full window/traffic-light chrome is not verified by these
images. Intel is compiled/signature-checked, not runtime-dogfooded. Manual drag/IME/VoiceOver,
HTTP/TLS/permissions fixtures and numerical resource/remote-soak budgets remain open.

Cloud callback validation is offline; it is not deployed sign-in evidence. Cloud remains
undeployed pending hosting/database selection, with Actions-only deployment at zmux.w3dev.app.
This receipt verifies the P1/P2/P3a first slices, not whole phases or complete cmux parity.

## Repairs discovered during hosted validation

| Run | Finding and repair |
| --- | --- |
| 38641714109 | Saved-cwd fixture used a /var versus /private/var alias; canonicalized the test directory. |
| 38642554863 | NSImage symbol initializer required an explicit weight on the runner SDK. |
| 38643467867 / 38644079733 | Added native predicate call-site/state diagnostics; ⇧⌘T synthesis discarded Shift. |
| 38644706173 | Canonicalized shifted menu equivalent and refreshed synthetic menu validation. See [AppKit equivalent rules](https://developer.apple.com/documentation/appkit/nsmenuitem/keyequivalentmodifiermask). |
| 38645312568 | Runtime tests passed, but downloaded visual review rejected a cramped browser pane. |
| 38646079812 | Snapshot SDK API is callback-only; adapted to a Sendable PNG continuation. |
| 38646603782 | Geometry assertion reproduced 104/1026 width. Replaced competing NSSplitView constraint ownership with an explicit frame host, accessible divider and bounded ratio cache. |
| 38692821187 | All selected checks, geometry assertions and downloaded visual review passed. |

Corrective source history: f6b60bf added scrolling/attention; 70cd3b6 removed an unused Swift
binding rejected by warnings-as-errors. All 45 workspace tests and the C ABI checks pass
locally and in the accepted macOS run. Physical trackpad feel remains a manual gate.
