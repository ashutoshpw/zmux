# Vendored vt100

Upstream: https://github.com/doy/vt100-rust
Registry version: 0.16.2, MIT. The original `LICENSE`, README, changelog, and
Cargo metadata are retained.
Verified crates.io archive SHA256:
`054ff75fb8fa83e609e685106df4faeffdf3a735d3c74ebce97ec557d5d36fd9`.

The local patch adds bounded per-cell OSC 8 hyperlink metadata to the upstream
terminal screen API. It keeps the upstream parser and screen mutation paths, so
erase, overwrite, scroll, resize, and alternate-screen behavior continue to be
owned by vt100. The hyperlink table is bounded; cells whose target ages out of
the table remain ordinary terminal text and never resolve to a different URL.
No cmux code or assets are included.
