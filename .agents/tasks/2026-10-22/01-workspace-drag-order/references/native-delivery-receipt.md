# Packaged workspace drag delivery receipt

The same-window workspace reorder fixture passed in the packaged native smoke.
This receipt pins the app, smoke report and retained source to one commit and
records the remaining gap in physical drag verification.
The machine-readable verifier output is retained in
[`native-delivery-receipt.json`](native-delivery-receipt.json).

## Artifact identity

- Source: `87dfbfe51bcbbb16289ecbf53e37e04f0ea01642`.
- [Successful macOS workflow run 39779731513](https://github.com/ashutoshpw/zmux/actions/runs/39779731513), job `119827593515`.
- [Universal artifact 12427877257](https://github.com/ashutoshpw/zmux/actions/runs/39779731513/artifacts/12427877257), named `zmux-macos-dev-universal-87dfbfe51bcbbb16289ecbf53e37e04f0ea01642`; expires 2026-11-05 13:05:38 UTC.
- Outer artifact: 23,322,682 bytes; API SHA-256 `a41fb19870bbeb4b144d99383bd62f34605f2d2e7c30997ce8c06035372ecc96`.
- Inner app ZIP: `zmux-dev-universal-87dfbfe.zip`, 20,280,574 bytes; SHA-256 `adea306a713be07347a0dbfc9051dd52a65d3e324987c04acd47ed4f40620d3f`.
- Build metadata: macOS 14+, universal arm64/x86_64, bundle ID `app.w3dev.zmux.dev`, ad-hoc signed and not notarized. The app executable, CLI and runtime library each contain both architecture slices. Hosted strict deep code-signature verification reported the bundle valid on disk and satisfying its designated requirement.
- All 341 source files listed in the packaged terminal-selection receipt match both the packaged snapshot and Git blobs at the pinned source commit.

## Verification results

The packaged smoke report has `success: true`, `workspace_order: true`, and
`workspace_order_drag: true`. All 74 required boolean checks from
`scripts/check-macos-app` are true. Its stage report is `assertions_complete`.
Strict Swift source validation, metadata compilation/contract checks, native
terminal-selection validation and hosted Rust/ABI checks passed. Terminal
selection matched all 41 expected cases and recorded both plain HTTP and OSC-8
loopback link requests.

The sidebar, status-menu, cloud-terminal lifecycle and native-window receipts
passed. The existing smoke fields are preserved exactly:
`native_save_dialog_accessibility: "skipped_user_deferred"`,
`browser_downloads_native: false`, and `browser_downloads_cancel_native: true`.

The inspected drag image is `zmux-workspace-drag.png` (SHA-256
`afbc82d67bf17e3be266be94c04278ed118caff2f5bdb876895c0f3d8d39b8d2`). Root's
visual inspection found a thin horizontal insertion marker at the top edge of
the sidebar, above the expanded workspace-group header. The Terminal view
remained selected with “2 local terminals”; Workspace2 and the temporary drag
workspace were visible below. This confirms the rendered insertion cue in the
capture, not physical gesture dispatch.

The packaged fixture captures the real row's source token on mouse-down and
sends synthetic `NSDraggingInfo` to the actual sidebar destination callbacks.
It verifies valid and rejected slots, grouped and collapsed destinations,
canonical and rendered order, and preservation of selection, responder, PTYs
and browser identity. It does not drive `mouseDragged` across AppKit's threshold,
call `beginDraggingSession`, exercise `NSDraggingSource` in the operating
system's drag loop, or verify a physical mouse/trackpad gesture. Those remain
manual native-dogfood gaps.

The artifact verifies application source at the pinned commit. The local
development-build bullet and task receipt were updated afterward; they are
documentation only and are not part of this artifact. The one-off geometry
probe workflow was removed after its exact 270-point result was retained in
[`scroll-geometry-probe-270.json`](scroll-geometry-probe-270.json).
