# App icon provenance

`AppIcon.png` is the unchanged transparent PNG supplied by the repository maintainer
on 2026-10-17 for use as the zmux logo. The earlier generated icon has been replaced.
No cmux artwork was imported. Upstream authorship and separate asset license details
were not supplied with the image.

The canonical image is 1254 × 1254 pixels in RGBA format. Its SHA-256 is
`aa76668d20bf69e583c15a1d5cf11ba79b7b0c8f24fe4374d36335bf4a8ef8ad`.

The macOS build uses Apple's `sips` and `iconutil` to produce all required ICNS sizes
while preserving alpha. The status menu draws that same bundled `AppIcon` with an
unread indicator. README references the canonical PNG directly.

`services/control-plane/public/` contains derived web assets: `logo.png` (256 × 256),
`apple-touch-icon.png` (180 × 180), and `favicon.ico` (16, 32, 48, and 64 pixels).
The PNGs were resized with Pillow's Lanczos filter; the ICO was encoded from the
canonical PNG using Pillow's ICO writer. Artwork, colors, and transparency are retained.
